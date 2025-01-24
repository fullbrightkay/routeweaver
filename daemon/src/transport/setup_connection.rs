use super::driver::{TransportReader, TransportWriter};
use crate::{
    noise::create_handshake_initiator,
    state::ServerState,
    transport::{
        packet::{Packet, PacketData},
        reader::packet_reader,
        writer::packet_writer,
    },
};
use arrayvec::ArrayVec;
use futures_util::{SinkExt, StreamExt, TryStreamExt};
use routeweaver_common::Peer;
use std::{pin::Pin, sync::Arc, time::Duration};
use tokio::{sync::mpsc, time::timeout};

#[derive(Debug, Default)]
pub struct PeerTracker {
    connected: scc::HashSet<Peer>,
}

impl PeerTracker {
    pub async fn add(&self, peer: Peer) {
        let _ = self.connected.insert_async(peer).await;
    }

    pub async fn remove(&self, peer: &Peer) {
        self.connected.remove_async(peer).await;
    }

    pub async fn is_connected(&self, peer: &Peer) -> bool {
        self.connected.contains_async(peer).await
    }
}

pub async fn finalize_peer_connection(
    server_state: Arc<ServerState>,
    reader: Option<impl TransportReader>,
    writer: Option<impl TransportWriter>,
    peer: Peer,
    initiator: bool,
) {
    if server_state.peer_tracker.is_connected(&peer).await {
        return;
    }

    tracing::debug!("Setting up connection for {}", peer);
    server_state.peer_tracker.add(peer).await;
    let _ = server_state.notification_new_peer_connection.send(peer);

    match (reader, writer) {
        (None, None) => {
            tracing::error!("Failed to accept connection from {}", peer);
            server_state.peer_tracker.remove(&peer).await;
        }
        (Some(reader), None) => {
            tokio::spawn(async move {
                tracing::debug!("Connection reader for {} starting", peer);

                packet_reader(server_state.clone(), Box::pin(reader), peer).await;
                server_state.peer_tracker.remove(&peer).await;

                tracing::debug!("Connection reader for {} closed", peer);
            });
        }
        (None, Some(writer)) => {
            let (request_write_packet_tx, request_write_packet_rx) = mpsc::channel(10);
            server_state
                .request_write_packet
                .upsert_async(peer, request_write_packet_tx)
                .await;

            tokio::spawn(async move {
                tracing::debug!("Connection writer for {} starting", peer);

                packet_writer(
                    server_state.clone(),
                    Box::pin(writer),
                    peer,
                    request_write_packet_rx,
                )
                .await;

                server_state.peer_tracker.remove(&peer).await;
                server_state.request_write_packet.remove_async(&peer).await;

                tracing::debug!("Connection writer for {} closed", peer);
            });
        }
        (Some(reader), Some(writer)) => {
            let (request_write_packet_tx, request_write_packet_rx) = mpsc::channel(10);
            server_state
                .request_write_packet
                .upsert_async(peer, request_write_packet_tx)
                .await;

            let mut reader = Box::pin(reader);
            let mut writer = Box::pin(writer);

            // Try doing an anonymous handshake here if we can
            // Note only the initator does the anonymous handshake to stop confusion
            if !server_state.anonymous && initiator {
                anonymous_handshake(&server_state, peer, &mut reader, &mut writer).await;
            }

            tokio::spawn(async move {
                tracing::debug!("Connection reader and writer for {} starting", peer);

                tokio::select! {
                    _ = packet_reader(server_state.clone(), reader, peer) => {}
                    _ = packet_writer(server_state.clone(), writer, peer, request_write_packet_rx) => {}
                }

                server_state.peer_tracker.remove(&peer).await;
                server_state.request_write_packet.remove_async(&peer).await;

                tracing::debug!("Connection reader and writer for {} closed", peer);
            });
        }
    }
}

/// Manually do part of the handshake logic here
///
/// This function must do the first handshake step, and get a response, so the handshake_tracker can actually store the thing
async fn anonymous_handshake(
    server_state: &ServerState,
    peer: Peer,
    reader: &mut Pin<Box<impl TransportReader>>,
    writer: &mut Pin<Box<impl TransportWriter>>,
) {
    let mut buffer = vec![0; 128];

    tracing::debug!("Attempting anonymous handshake with peer {}", peer);

    let mut handshake_state = create_handshake_initiator(&server_state.keys.private);

    if let Ok(amount) = handshake_state.write_message(&[], &mut buffer) {
        let packet = Packet {
            source: server_state.keys.public,
            destination: None,
            data: PacketData::Handshake(
                ArrayVec::try_from(&buffer[..amount]).expect("This shouldn't happen"),
            ),
        };

        // Do the first step,
        match timeout(Duration::from_secs(10), writer.send(packet)).await {
            Ok(Ok(_)) => {
                // Best scenario, they accepted our packet (probably)
            }
            Ok(Err(err)) => {
                // Worst scenario, they closed the connection over this
                todo!()
            }
            Err(_) => {
                // Timeout occured, they are probably operating in anonymous mode
                tracing::info!(
                    "Anonymous handshake with peer {} failed, this is not vital",
                    peer
                );
            }
        }

        // Read the next packet and interpret it

        match timeout(Duration::from_secs(10), reader.try_next()).await {
            Ok(Ok(Some(packet))) => {
                // FIXME: This is dropping a packet, while this isn't life ending, it is kind of bad.
                // In the future, we need the readable to be peekable
                if packet.destination != Some(server_state.keys.public) {
                    // Remote didn't care
                    return;
                }

                if let PacketData::Handshake(data) = packet.data {
                    if handshake_state.read_message(&data, &mut buffer).is_err() {
                        // Remote didn't care
                        return;
                    }

                    tracing::info!(
                        "Node {} seems to have cared about the anonymous handshake, storing",
                        packet.source
                    );

                    let _ = server_state
                        .handshake_tracker
                        .upsert_async(packet.source, handshake_state)
                        .await;
                }
            }
            Ok(Ok(None)) | Ok(Err(_)) => {
                // Connection was closed
            }
            Err(_) => {
                // Remote didn't seem to care
            }
        }
    }
}
