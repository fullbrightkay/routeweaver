use crate::{
    noise::create_handshake_initiator,
    state::ServerState,
    transport::{
        packet::{Packet, PacketData},
        router::RequestRoutePacket,
    },
};
use routeweaver_common::PublicKey;
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn channel_initiator(
    server_state: Arc<ServerState>,
    mut request_initiate_channel: mpsc::Receiver<PublicKey>,
) {
    let mut buffer = vec![0; 128];

    while let Some(node) = request_initiate_channel.recv().await {
        if server_state.handshake_tracker.contains_async(&node).await {
            continue;
        }

        let mut handshake_state = create_handshake_initiator(&server_state.keys.private);

        if let Err(err) = handshake_state.write_message(&[], &mut buffer) {
            tracing::info!(
                "Failed to write initial handshake message to {} due to {}",
                node,
                err
            );
        } else {
            let packet = Packet {
                source: server_state.keys.public,
                destination: Some(node),
                data: PacketData::Handshake(buffer.as_slice().try_into().unwrap()),
            };

            server_state
                .request_route_packet
                .send(RequestRoutePacket {
                    origin: None,
                    packet,
                })
                .await
                .unwrap();
        }
    }
}
