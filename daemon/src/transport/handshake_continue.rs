use arrayvec::ArrayVec;
use routeweaver_common::PublicKey;
use snow::TransportState;
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;

use crate::state::ServerState;

use super::{
    packet::{Packet, PacketData},
    router::RequestRoutePacket,
};

pub async fn handshake_continue(server_state: Arc<ServerState>) {
    let mut buffer = vec![0; 128];

    loop {
        let mut entry = server_state.handshake_tracker.first_entry_async().await;

        while let Some(mut handshake_state) = entry {
            let source = *handshake_state.key();

            tracing::debug!(
                "Attempting to continue handshake with {}",
                handshake_state.key()
            );

            if handshake_state.is_handshake_finished() {
                let transport_state: TransportState = handshake_state.remove().try_into().unwrap();

                let remote_node_id = PublicKey::new(
                    transport_state
                        .get_remote_static()
                        .unwrap()
                        .try_into()
                        .unwrap(),
                );

                if source != remote_node_id {
                    tracing::error!(
                        "Node claims it is {} but during handshake was discovered to be {} instead",
                        source,
                        remote_node_id
                    );
                } else {
                    tracing::debug!("Finalized handshake with node {}", source);

                    server_state
                        .transport_tracker
                        .upsert_async(source, transport_state)
                        .await;
                }

                break;
            } else if handshake_state.is_my_turn() {
                if let Ok(amount) = handshake_state.write_message(&[], &mut buffer) {
                    let packet = Packet {
                        source: server_state.keys.public,
                        destination: Some(source),
                        data: PacketData::Handshake(ArrayVec::try_from(&buffer[..amount]).unwrap()),
                    };

                    server_state
                        .request_route_packet
                        .send(RequestRoutePacket {
                            origin: None,
                            packet,
                        })
                        .await
                        .unwrap();
                } else {
                    // FIXME: this cuts the loop short
                    let _ = handshake_state.remove();
                    break;
                }
            }

            entry = handshake_state.next_async().await;
        }

        sleep(Duration::from_secs(1)).await;
    }
}
