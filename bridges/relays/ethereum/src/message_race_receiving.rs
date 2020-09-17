// Copyright 2019-2020 Parity Technologies (UK) Ltd.
// This file is part of Parity Bridges Common.

// Parity Bridges Common is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity Bridges Common is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

use crate::message_lane::{MessageLane, SourceHeaderIdOf, TargetHeaderIdOf};
use crate::message_lane_loop::{
	SourceClient as MessageLaneSourceClient, SourceClientState, TargetClient as MessageLaneTargetClient,
	TargetClientState,
};
use crate::message_race_delivery::DeliveryStrategy;
use crate::message_race_loop::{MessageRace, SourceClient, TargetClient};
use crate::utils::FailedClient;

use async_trait::async_trait;
use futures::stream::FusedStream;
use std::{marker::PhantomData, ops::RangeInclusive, time::Duration};

/// Message receiving confirmations delivery strategy.
type ReceivingConfirmationsDeliveryStrategy<P> = DeliveryStrategy<
	<P as MessageLane>::TargetHeaderNumber,
	<P as MessageLane>::TargetHeaderHash,
	<P as MessageLane>::SourceHeaderNumber,
	<P as MessageLane>::SourceHeaderHash,
	<P as MessageLane>::MessageNonce,
	<P as MessageLane>::MessagesReceivingProof,
>;

/// Run receiving confirmations race.
pub async fn run<P: MessageLane>(
	source_client: impl MessageLaneSourceClient<P>,
	source_state_updates: impl FusedStream<Item = SourceClientState<P>>,
	target_client: impl MessageLaneTargetClient<P>,
	target_state_updates: impl FusedStream<Item = TargetClientState<P>>,
	stall_timeout: Duration,
) -> Result<(), FailedClient> {
	crate::message_race_loop::run(
		ReceivingConfirmationsRaceSource {
			client: target_client,
			_phantom: Default::default(),
		},
		target_state_updates,
		ReceivingConfirmationsRaceTarget {
			client: source_client,
			_phantom: Default::default(),
		},
		source_state_updates,
		stall_timeout,
		ReceivingConfirmationsDeliveryStrategy::<P>::new(std::u32::MAX.into()),
	)
	.await
}

/// Messages receiving confirmations race.
struct ReceivingConfirmationsRace<P>(std::marker::PhantomData<P>);

impl<P: MessageLane> MessageRace for ReceivingConfirmationsRace<P> {
	type SourceHeaderId = TargetHeaderIdOf<P>;
	type TargetHeaderId = SourceHeaderIdOf<P>;

	type MessageNonce = P::MessageNonce;
	type Proof = P::MessagesReceivingProof;

	fn source_name() -> String {
		format!("{}::ReceivingConfirmationsDelivery", P::SOURCE_NAME)
	}

	fn target_name() -> String {
		format!("{}::ReceivingConfirmationsDelivery", P::TARGET_NAME)
	}
}

/// Message receiving confirmations race source, which is a target of the lane.
struct ReceivingConfirmationsRaceSource<P: MessageLane, C> {
	client: C,
	_phantom: PhantomData<P>,
}

#[async_trait(?Send)]
impl<P, C> SourceClient<ReceivingConfirmationsRace<P>> for ReceivingConfirmationsRaceSource<P, C>
where
	P: MessageLane,
	C: MessageLaneTargetClient<P>,
{
	type Error = C::Error;

	async fn latest_nonce(
		&self,
		at_block: TargetHeaderIdOf<P>,
	) -> Result<(TargetHeaderIdOf<P>, P::MessageNonce), Self::Error> {
		self.client.latest_received_nonce(at_block).await
	}

	async fn generate_proof(
		&self,
		at_block: TargetHeaderIdOf<P>,
		nonces: RangeInclusive<P::MessageNonce>,
	) -> Result<
		(
			TargetHeaderIdOf<P>,
			RangeInclusive<P::MessageNonce>,
			P::MessagesReceivingProof,
		),
		Self::Error,
	> {
		self.client
			.prove_messages_receiving(at_block)
			.await
			.map(|(at_block, proof)| (at_block, nonces, proof))
	}
}

/// Message receiving confirmations race target, which is a source of the lane.
struct ReceivingConfirmationsRaceTarget<P: MessageLane, C> {
	client: C,
	_phantom: PhantomData<P>,
}

#[async_trait(?Send)]
impl<P, C> TargetClient<ReceivingConfirmationsRace<P>> for ReceivingConfirmationsRaceTarget<P, C>
where
	P: MessageLane,
	C: MessageLaneSourceClient<P>,
{
	type Error = C::Error;

	async fn latest_nonce(
		&self,
		at_block: SourceHeaderIdOf<P>,
	) -> Result<(SourceHeaderIdOf<P>, P::MessageNonce), Self::Error> {
		self.client.latest_confirmed_received_nonce(at_block).await
	}

	async fn submit_proof(
		&self,
		generated_at_block: TargetHeaderIdOf<P>,
		_nonces: RangeInclusive<P::MessageNonce>,
		proof: P::MessagesReceivingProof,
	) -> Result<RangeInclusive<P::MessageNonce>, Self::Error> {
		self.client
			.submit_messages_receiving_proof(generated_at_block, proof)
			.await
	}
}