use super::{PacketEncoderDecoder, Transport, TransportReader, TransportWriter};
use crate::error::RouteWeaverError;
use routeweaver_common::{Address, Protocol};
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use socket2::Socket;
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv6Addr, SocketAddr},
};
use sysinfo::Networks;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::{FramedRead, FramedWrite};

#[serde_inline_default]
#[derive(Clone, Deserialize, Debug)]
pub struct TcpTransportConfig {
    #[serde_inline_default(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0)))]
    pub listen_address: IpAddr,
    #[serde_inline_default(3434)]
    pub listen_port: u16,
}

pub struct TcpTransport {
    socket: TcpListener,
    listen_port: u16,
}

impl Transport for TcpTransport {
    const PROTOCOL: Protocol = Protocol::Tcp;

    async fn from_config(config: toml::Value) -> Result<Self, RouteWeaverError> {
        let config = TcpTransportConfig::deserialize(config)?;

        let listen_address = match config.listen_address {
            IpAddr::V4(ip) => IpAddr::V6(ip.to_ipv6_mapped()),
            IpAddr::V6(ip) => IpAddr::V6(ip),
        };

        let socket = Socket::new(
            socket2::Domain::IPV6,
            socket2::Type::STREAM,
            Some(socket2::Protocol::TCP),
        )?;

        socket.set_only_v6(false)?;
        socket.set_nonblocking(true)?;
        socket.set_reuse_address(true)?;

        socket.bind(&SocketAddr::new(listen_address, config.listen_port).into())?;
        socket.listen(2)?;

        Ok(Self {
            socket: TcpListener::from_std(socket.into())?,
            listen_port: config.listen_port,
        })
    }

    async fn connect(
        &self,
        address: &Address,
    ) -> Result<(Option<impl TransportReader>, Option<impl TransportWriter>), RouteWeaverError>
    {
        let socket_addr: SocketAddr = address.clone().try_into().unwrap();

        Ok(TcpStream::connect(socket_addr).await.map(|stream| {
            let (read, write) = stream.into_split();
            (
                Some(FramedRead::new(read, PacketEncoderDecoder)),
                Some(FramedWrite::new(write, PacketEncoderDecoder)),
            )
        })?)
    }

    async fn accept(
        &self,
    ) -> Result<
        (
            (Option<impl TransportReader>, Option<impl TransportWriter>),
            Address,
        ),
        RouteWeaverError,
    > {
        Ok(self.socket.accept().await.map(|(stream, address)| {
            let (read, write) = stream.into_split();
            (
                (
                    Some(FramedRead::new(read, PacketEncoderDecoder)),
                    Some(FramedWrite::new(write, PacketEncoderDecoder)),
                ),
                address.into(),
            )
        })?)
    }

    async fn local_addresses(&self) -> Result<impl Iterator<Item = Address>, RouteWeaverError> {
        let mut ip_addrs = HashSet::new();
        let networks = Networks::new_with_refreshed_list();

        for (_, network_data) in &networks {
            ip_addrs.extend(
                network_data
                    .ip_networks()
                    .iter()
                    .map(|ip_network| ip_network.addr),
            );
        }

        Ok(ip_addrs
            .into_iter()
            .filter(|ip_addr| !ip_addr.is_loopback())
            .map(|ip_addr| Address::Ip {
                address: ip_addr,
                port: self.listen_port,
            }))
    }
}
