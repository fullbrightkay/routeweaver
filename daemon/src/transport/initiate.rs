use super::driver::Transport;
use crate::{state::ServerState, transport::setup_connection::finalize_peer_connection};
use routeweaver_common::{Address, Peer};
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc, time::timeout};

pub async fn connection_initiator<T: Transport>(
    server_state: Arc<ServerState>,
    transport: Arc<T>,
    mut request_initiate_connection: mpsc::Receiver<Address>,
) {
    while let Some(address) = request_initiate_connection.recv().await {
        let peer = Peer {
            protocol: T::PROTOCOL,
            address,
        };

        tracing::debug!("Initiating connection to {}", peer);

        match timeout(Duration::from_secs(10), transport.connect(&address)).await {
            Ok(Ok((reader, writer))) => {
                finalize_peer_connection(server_state.clone(), reader, writer, peer, true).await;
            }
            _ => {
                tracing::warn!("Failed to initiate connection to {}", peer);
            }
        }
    }
}
