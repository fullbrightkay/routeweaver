use routeweaver_common::Peer;

use super::{driver::Transport, setup_connection::finalize_peer_connection};
use crate::state::ServerState;
use std::sync::Arc;

pub async fn accepter<T: Transport>(server_state: Arc<ServerState>, transport: Arc<T>) {
    loop {
        match transport.accept().await {
            Ok(((reader, writer), address)) => {
                let peer = Peer {
                    address,
                    protocol: T::PROTOCOL,
                };

                tracing::debug!("Accepted connection from {}", peer);

                finalize_peer_connection(server_state.clone(), reader, writer, peer, false).await;
            }
            Err(err) => {
                tracing::warn!("Failed to accept connection: {}", err);
            }
        }
    }
}
