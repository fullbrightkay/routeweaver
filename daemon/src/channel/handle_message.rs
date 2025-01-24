use crate::{proto::Message, state::ServerState};
use routeweaver_common::PublicKey;
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;

use super::writer::RequestWriteMessage;

pub async fn handle_message(server_state: Arc<ServerState>, node: PublicKey, message: Message) {
    match message {
        Message::RequestPeerSuggestion => {
            // For now "suggest" our local addresses. Very dumb but anything better isn't possible
            // until a more complete routing system is in place
            let suggested_peers = server_state.local_address_tracker.iter().await.collect();
            let response = Message::PeerSuggestion {
                peers: suggested_peers,
            };

            server_state
                .request_write_message
                .send(RequestWriteMessage {
                    notify_sent: None,
                    destination: node,
                    message: response,
                })
                .await
                .unwrap();
        }
        Message::PeerSuggestion { peers } => {
            // Spawn off a task that SLOWLY feeds these peers into the system, giving grace time so
            // a malicious node can't dos a set of nodes using us
            tokio::spawn(async move {
                for peer in peers {
                    if let Some(initator) = server_state
                        .request_initiate_connection
                        .get_async(&peer.protocol)
                        .await
                    {
                        tracing::debug!("Attempting to connect to suggested peer {}", peer);

                        initator.send(peer.address).await.unwrap();
                    } else {
                        tracing::debug!(
                            "Got peer using protocol {} from node {}, but this protocol is not active",
                            node,
                            peer.protocol
                        );
                    }

                    sleep(Duration::from_secs(10)).await;
                }
            });
        }
        Message::RequestConnection { application } => todo!(),
        Message::ConnectionAccepted {
            application,
            connection_id,
        } => todo!(),
        Message::ConnectionDenied { application } => todo!(),
        Message::ConnectionHeartbeat { connection_id } => todo!(),
        Message::ConnectionClose { connection_id } => {
            if server_state
                .request_receive_connection_data
                .remove_async(&connection_id)
                .await
                .is_none()
            {
                tracing::warn!(
                    "Node {} tried closing connection {}, but this connection did not exist",
                    node,
                    connection_id
                );
            }
        }
        Message::ConnectionData {
            connection_id,
            data,
        } => {
            if let Some(entry) = server_state
                .request_receive_connection_data
                .get_async(&connection_id)
                .await
            {
                match entry.send(data).await {
                    Ok(_) => {
                        tracing::info!("Node {} sent data over connection {}", node, connection_id);
                    }
                    Err(_) => {
                        tracing::warn!("Node {} sent data over connection {}, but this connection does not exist", node, connection_id);
                        let _ = entry.remove_entry();
                    }
                }
            }
        }
    }
}
