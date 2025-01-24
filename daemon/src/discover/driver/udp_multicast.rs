use super::Discovery;
use crate::error::RouteWeaverError;
use futures_util::{stream::try_unfold, Stream};
use routeweaver_common::Peer;
use socket2::Socket;
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
};
use tokio::net::UdpSocket;

const BROADCAST_ADDRESS_V4: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 255);
const BROADCAST_SOCKET_V4: SocketAddr = SocketAddr::new(IpAddr::V4(BROADCAST_ADDRESS_V4), 4343);

const BROADCAST_ADDRESS_V6: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);
const BROADCAST_SOCKET_V6: SocketAddr = SocketAddr::new(IpAddr::V6(BROADCAST_ADDRESS_V6), 4343);

pub struct UdpMulticastDiscoveryConfig {}

pub struct UdpMulticastDiscovery {
    v4_socket: Arc<UdpSocket>,
    v6_socket: Arc<UdpSocket>,
}

impl Discovery for UdpMulticastDiscovery {
    const ID: &'static str = "udp-multicast";

    async fn from_config(_config: toml::Value) -> Result<Self, RouteWeaverError> {
        let v4_socket = Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::DGRAM,
            Some(socket2::Protocol::UDP),
        )?;

        v4_socket.set_nonblocking(true)?;
        v4_socket.set_broadcast(true)?;
        v4_socket.bind(&SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 4343).into())?;

        let v6_socket = Socket::new(
            socket2::Domain::IPV6,
            socket2::Type::DGRAM,
            Some(socket2::Protocol::UDP),
        )?;

        v6_socket.set_only_v6(true)?;
        v6_socket.set_nonblocking(true)?;
        v6_socket.bind(&SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 4343).into())?;

        v6_socket.set_multicast_loop_v6(false)?;
        v6_socket.set_multicast_hops_v6(4)?;
        v6_socket.join_multicast_v6(&BROADCAST_ADDRESS_V6, 0)?;

        Ok(Self {
            v4_socket: Arc::new(UdpSocket::from_std(v4_socket.into())?),
            v6_socket: Arc::new(UdpSocket::from_std(v6_socket.into())?),
        })
    }

    async fn announce(
        &self,
        local_addresses: impl Iterator<Item = Peer> + Send,
    ) -> Result<(), RouteWeaverError> {
        let mut buffer = Vec::new();

        for address in local_addresses {
            buffer.clear();
            bincode::serde::encode_into_std_write(
                address,
                &mut buffer,
                bincode::config::standard(),
            )?;
            self.v4_socket.send_to(&buffer, BROADCAST_SOCKET_V4).await?;
            self.v6_socket.send_to(&buffer, BROADCAST_SOCKET_V6).await?;
        }

        Ok(())
    }

    async fn discover(
        &self,
    ) -> Result<impl Stream<Item = Result<Peer, RouteWeaverError>> + Send, RouteWeaverError> {
        let buffer = Vec::new();
        let socket = self.v6_socket.clone();

        Ok(try_unfold(
            (socket, buffer),
            move |(socket, mut buffer)| async move {
                loop {
                    buffer.clear();

                    // We don't throw an error here cuz we want to keep trying
                    let Ok(amount) = socket.recv_buf(&mut buffer).await else {
                        continue;
                    };

                    let Ok((peer, _)) = bincode::serde::decode_from_slice(
                        &buffer[..amount],
                        bincode::config::standard(),
                    ) else {
                        tracing::warn!("Failed to decode announcement");

                        continue;
                    };

                    return Ok(Some((peer, (socket, buffer))));
                }
            },
        ))
    }
}
