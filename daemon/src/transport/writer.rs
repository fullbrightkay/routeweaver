use super::driver::TransportWriter;
use crate::{state::ServerState, transport::packet::Packet};
use futures_util::SinkExt;
use routeweaver_common::Peer;
use std::{pin::Pin, sync::Arc};
use tokio::sync::mpsc;

pub async fn packet_writer(
    server_state: Arc<ServerState>,
    mut peer_connection: Pin<Box<impl TransportWriter>>,
    peer: Peer,
    mut request_write_packet: mpsc::Receiver<Packet>,
) {
    while let Some(packet) = request_write_packet.recv().await {
        tracing::debug!(
            "Writing packet through {} that goes from {} to {:?}",
            peer,
            packet.source,
            packet.destination
        );

        if let Err(err) = peer_connection.send(packet).await {
            tracing::warn!("Failed to write packet through {}: {}", peer, err);

            break;
        }
    }
}
