use super::packet::Packet;
use crate::state::ServerState;
use rand::{prelude::StdRng, Rng, SeedableRng};
use ringbuffer::{ConstGenericRingBuffer, RingBuffer};
use routeweaver_common::Peer;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::mpsc, time::sleep};

const LOCAL_OUTBOUND_PACKET_TIMEOUT: Duration = Duration::from_secs(5);
const REMOTE_OUTBOUND_PACKET_TIMEOUT: Duration = Duration::from_secs(1);

pub struct RequestRoutePacket {
    /// This is tracked so that we cannot route back to the sender, preventing infinite loops
    pub origin: Option<Peer>,
    /// Packet intended to go somewhere
    pub packet: Packet,
}

enum PeerEvent {
    AcceptedPacket { time_taken: Duration, failed: bool },
}

impl PeerEvent {
    pub fn score(&self) -> f32 {
        match self {
            PeerEvent::AcceptedPacket { time_taken, failed } => {
                time_taken.as_secs_f32() * if *failed { -1.0 } else { 1.0 }
            }
        }
    }
}

#[derive(Default)]
struct ScoreKeeper(HashMap<Peer, ConstGenericRingBuffer<PeerEvent, 50>>);

impl ScoreKeeper {
    fn insert(&mut self, peer: Peer, event: PeerEvent) {
        self.0.entry(peer).or_default().push(event);
    }

    fn best(&self) -> Option<Peer> {
        self.0
            .iter()
            .max_by(|(_, a), (_, b)| {
                a.iter()
                    .map(|a| a.score())
                    .sum::<f32>()
                    .total_cmp(&b.iter().map(|b| b.score()).sum())
            })
            .map(|(k, _)| *k)
    }
}

// FIXME: This is essentially me playing around with algos and is obscenely inefficient

/// Delegates packets to a calculated "most correct" peer
pub async fn packet_router(
    server_state: Arc<ServerState>,
    mut request_route_packet: mpsc::Receiver<RequestRoutePacket>,
) {
    let score_keeper = ScoreKeeper::default();
    let mut new_canidates = VecDeque::new();
    let mut rng = StdRng::from_entropy();
    let mut notification_new_peer_connection =
        server_state.notification_new_peer_connection.subscribe();

    while let Some(RequestRoutePacket { origin, packet }) = request_route_packet.recv().await {
        new_canidates.clear();

        if let Some(destination) = packet.destination {
            let mut tries_until_return_to_sender_allowed: u8 = 5;

            // TODO: as a implementation hack, chose a random route
            loop {
                let current_canidate = 
                // 10% of the time try to grab from the newly seen canidates
                if rng.gen_ratio(10, 100) {
                    new_canidates.pop_front()

                // Otherwise try to grab the canidate with the highest score
                } else {
                    score_keeper.best()
                };

                if let Some(peer) = current_canidate {
                    tracing::debug!(
                        "Attempting to route packet through {} from {} to {}",
                        peer,
                        packet.source,
                        destination
                    );

                    if let Some(entry) = server_state.request_write_packet.get_async(&peer).await {
                        // Don't send back to whoever handed it to us unless we really have to
                        if Some(*entry.key()) != origin || tries_until_return_to_sender_allowed == 0
                        {
                            match entry.get().send(packet.clone()).await {
                                Ok(_) => {
                                    tracing::debug!(
                                        "Routed packet through {} from {} to {}",
                                        entry.key(),
                                        packet.source,
                                        destination
                                    );
                                    break;
                                }
                                Err(err) => {
                                    tracing::error!(
                                        "Failed to route packet through {} from {} to {}: {}",
                                        entry.key(),
                                        packet.source,
                                        destination,
                                        err
                                    );

                                    // If a send error occured here that means the task most likely does not exist any more
                                    let _ = entry.remove_entry();
                                    continue;
                                }
                            }
                        }
                    }


                }

                tracing::debug!(
                    "Failed to select a canidate to route a packet from {} to {} through",
                    packet.source,
                    destination
                );

                tries_until_return_to_sender_allowed = tries_until_return_to_sender_allowed.saturating_sub(1);

                // Wait around either for a new peer connection or for a period of time to pass
                tokio::select! {
                    // We prefer a new peer connection
                    biased;

                    peer = notification_new_peer_connection.recv() => {
                        if let Ok(peer) = peer {
                            new_canidates.push_back(peer);
                        }
                    }
                    _ = sleep(Duration::from_millis(10)) => {}
                }
            }
        } else {
            tracing::error!("Packet with anonymous destination was erroneously sent to the router, this is a major bug");
        }
    }
}
