use crate::{state::ServerState, transport::driver::Transport};
use driver::Discovery;
use futures_util::StreamExt;
use routeweaver_common::{Address, Peer, Protocol};
use std::{collections::HashSet, pin::pin, sync::Arc, time::Duration};
use tokio::time::sleep;

pub mod driver;

#[derive(Debug, Default)]
pub struct LocalAddressTracker {
    addresses: scc::HashMap<Protocol, HashSet<Address>>,
}

impl LocalAddressTracker {
    pub async fn replace(&self, protocol: Protocol, address: impl IntoIterator<Item = Address>) {
        self.addresses
            .upsert_async(protocol, address.into_iter().collect())
            .await;

        for peer in self.addresses.get_async(&protocol).await.unwrap().iter() {
            tracing::debug!("Added local address address {}", peer);
        }
    }

    /// TODO: This is stupid
    pub async fn iter(&self) -> impl Iterator<Item = Peer> + use<'_> {
        let mut values = Vec::new();

        self.addresses
            .scan_async(|protocol, value| {
                values.extend(value.iter().copied().map({
                    |address| Peer {
                        address,
                        protocol: *protocol,
                    }
                }));
            })
            .await;

        values.into_iter()
    }
}

pub async fn announcer<D: Discovery>(server_state: Arc<ServerState>, discovery: Arc<D>) {
    loop {
        tracing::debug!("Announcing local addresses over discovery {}", D::ID);

        discovery
            .announce(server_state.local_address_tracker.iter().await)
            .await
            .unwrap();

        sleep(Duration::from_secs(60)).await;
    }
}

pub async fn local_address_refresher<T: Transport>(
    server_state: Arc<ServerState>,
    transport: Arc<T>,
) {
    loop {
        tracing::debug!("Refreshing local addresses for transport {}", T::PROTOCOL);

        if let Ok(local_addresses) = transport.local_addresses().await {
            server_state
                .local_address_tracker
                .replace(T::PROTOCOL, local_addresses)
                .await;
        }

        sleep(Duration::from_secs(120)).await;
    }
}

pub async fn discoverer<D: Discovery>(server_state: Arc<ServerState>, discovery: Arc<D>) {
    if let Ok(mut peers) = discovery.discover().await {
        let mut peers = pin!(peers);
        tracing::debug!("Discovering peers over discovery {}", D::ID);

        while let Some(peer) = peers.next().await {
            match peer {
                Ok(peer) => {
                    if peer.is_loopback() {
                        tracing::debug!("Skipping loopback peer {}", peer);
                        continue;
                    }

                    if let Some(sender) =
                        server_state.request_initiate_connection.get(&peer.protocol)
                    {
                        sender.send(peer.address).await.unwrap();
                    } else {
                        tracing::warn!("Protocol {} not supported", peer.protocol);
                    }
                }
                Err(err) => {
                    tracing::warn!("Failed to discover peer: {}", err);
                    continue;
                }
            }
        }
    }

    tracing::warn!("Discovery over {} stopped", D::ID);
}
