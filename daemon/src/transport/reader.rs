use super::{driver::TransportReader, packet::Packet, router::RequestRoutePacket};
use crate::{
    channel::reader::RequestDecodeMessageSegment,
    noise::create_handshake_responder,
    state::ServerState,
    transport::packet::{MessageSegment, PacketData},
};
use arrayvec::ArrayVec;
use futures_util::StreamExt;
use routeweaver_common::{Peer, PublicKey};
use snow::{HandshakeState, TransportState};
use std::{pin::Pin, sync::Arc};

/// Reads packets from the transport, decodes them, and sends the results to the relevant bins
pub async fn packet_reader(
    server_state: Arc<ServerState>,
    mut peer_connection: Pin<Box<impl TransportReader>>,
    peer: Peer,
) {
    let mut encryption_buffer = vec![0; u16::MAX as usize];

    while let Some(packet) = peer_connection.next().await {
        match packet {
            Ok(packet) => {
                // Someone is pretending to be us or this is from this machine
                if packet.source == server_state.keys.public {
                    tracing::warn!("Received packet from self, discarding");
                    continue;
                }

                // Someone is trying to ping us, reject it as this is not proper usage
                if packet.destination == Some(packet.source) {
                    tracing::warn!(
                        "Packet intends to travel to its source {}, discarding",
                        packet.source
                    );
                    continue;
                }

                // It's for us, and if the destination is [Option::None] its our peer speaking to us
                if packet.destination == Some(server_state.keys.public)
                    || packet.destination.is_none()
                {
                    tracing::debug!("Received packet for us from {}", packet.source);

                    match packet.data {
                        // Someone with the intention to make a handshake
                        PacketData::Handshake(data) => {
                            tracing::debug!("Received handshake from {}", packet.source);

                            let handshake_state_guard = server_state
                                .handshake_tracker
                                .entry_async(packet.source)
                                .await
                                .or_insert_with(|| {
                                    create_handshake_responder(&server_state.keys.private)
                                });

                            handle_handshake(packet.source, data, handshake_state_guard).await;
                        }
                        PacketData::MessageSegment(data) => {
                            if packet.destination.is_none() {
                                tracing::warn!("Packet from {} is being sent to anonymous destination yet is not a handshake packet, discarding", packet.source);
                            }

                            let Some(mut transport_state) = server_state
                                .transport_tracker
                                .get_async(&packet.source)
                                .await
                            else {
                                tracing::warn!(
                                    "Got message segment from node {} without channel up",
                                    packet.source
                                );
                                
                                server_state
                                    .request_initiate_channel
                                    .send(packet.source)
                                    .await
                                    .unwrap();

                                continue;
                            };

                            match transport_state.read_message(&data, &mut encryption_buffer) {
                                Ok(amount) => {
                                    let (segment, _): (MessageSegment, _) =
                                        bincode::serde::decode_from_slice(
                                            &encryption_buffer[..amount],
                                            bincode::config::standard(),
                                        )
                                        .unwrap();

                                    server_state
                                        .request_decode_message_segment
                                        .send(RequestDecodeMessageSegment {
                                            source: packet.source,
                                            segment,
                                        })
                                        .await
                                        .unwrap();
                                }
                                Err(err) => {
                                    tracing::error!("Error reading message segment: {}", err);
                                }
                            }
                        }
                    }
                // Its for someone else
                } else {
                    tracing::debug!(
                        "Received packet from {} going to {}",
                        packet.source,
                        packet.destination.unwrap()
                    );

                    server_state
                        .request_route_packet
                        .send(RequestRoutePacket {
                            origin: Some(peer),
                            packet,
                        })
                        .await
                        .unwrap();
                }
            }
            Err(err) => {
                tracing::error!("Connection reader for {} encountered error: {}", peer, err);
                break;
            }
        }
    }
}

async fn handle_handshake(
    source: PublicKey,
    data: ArrayVec<u8, 128>,
    mut handshake_state_guard: scc::hash_map::OccupiedEntry<'_, PublicKey, HandshakeState>,
) {
    let handshake_state = handshake_state_guard.get_mut();

    if handshake_state.is_my_turn() || handshake_state.is_handshake_finished() {
        // Ignore this handshake state
        return;
    }

    // Attempt to decode and validate it
    match handshake_state.read_message(&data, &mut []) {
        Ok(amount) => {
            if amount != 0 {
                tracing::error!("Node {} sent incorrect handshake message", source);
                let _ = handshake_state_guard.remove();
            }
        }
        // Failure means remove the entry
        Err(err) => {
            tracing::error!(
                "Error decoding handshake message from node {}: {}",
                source,
                err
            );
            let _ = handshake_state_guard.remove();
        }
    }
}
