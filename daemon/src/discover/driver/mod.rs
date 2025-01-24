#[cfg(discovery_bluetooth_passive)]
pub mod bluetooth_passive;

#[cfg(discovery_udp_multicast)]
pub mod udp_multicast;

use crate::error::RouteWeaverError;
use futures_util::Stream;
use routeweaver_common::Peer;

pub trait Discovery: Sized + Send + Sync + 'static {
    const ID: &'static str;

    async fn from_config(config: toml::Value) -> Result<Self, RouteWeaverError>;

    async fn announce(
        &self,
        local_addresses: impl Iterator<Item = Peer> + Send,
    ) -> Result<(), RouteWeaverError>;

    // Usually goes forever
    async fn discover(
        &self,
    ) -> Result<impl Stream<Item = Result<Peer, RouteWeaverError>> + Send, RouteWeaverError>;
}
