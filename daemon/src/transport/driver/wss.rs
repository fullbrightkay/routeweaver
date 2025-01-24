use super::{Transport, TransportReader, TransportWriter};
use crate::error::RouteWeaverError;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt, TryStreamExt};
use routeweaver_common::{Address, Protocol};
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use socket2::Socket;
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::LazyLock,
};
use sysinfo::Networks;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::{protocol::WebSocketConfig, Message};
use url::Url;

static WEB_SOCKET_CONFIG: LazyLock<WebSocketConfig> = LazyLock::new(WebSocketConfig::default);

#[serde_inline_default]
#[derive(Clone, Deserialize, Debug)]
pub struct WssTransportConfig {
    #[serde_inline_default(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0)))]
    pub listen_address: IpAddr,
    #[serde_inline_default(3436)]
    pub listen_port: u16,
}

pub struct WssTransport {
    socket: TcpListener,
    listen_port: u16,
}

impl Transport for WssTransport {
    const PROTOCOL: Protocol = Protocol::Wss;

    async fn from_config(config: toml::Value) -> Result<Self, RouteWeaverError> {
        let config = WssTransportConfig::deserialize(config)?;

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
        let url = Url::parse(&format!("wss://{}", address))
            .map_err(|_| RouteWeaverError::InvalidProtocol)?;

        let (stream, _) = tokio_tungstenite::connect_async_tls_with_config(
            url,
            Some(*WEB_SOCKET_CONFIG),
            false,
            None,
        )
        .await
        .map_err(|_| RouteWeaverError::ConnectionFailed)?;
        let (write, read) = stream.split();

        let read = read
            .map_err(|_| RouteWeaverError::UnknownPacketDecoding)
            .try_filter_map(|message| async move {
                match message {
                    Message::Binary(bytes) => {
                        let (packet, _) =
                            bincode::serde::decode_from_slice(&bytes, bincode::config::standard())?;

                        Ok(Some(packet))
                    }
                    Message::Close(_) => Ok(None),
                    _ => Err(RouteWeaverError::UnknownPacketDecoding),
                }
            });

        let write = write
            .sink_map_err(|_| RouteWeaverError::UnknownPacketDecoding)
            .with(move |packet| async move {
                Ok(Message::Binary(Bytes::from_owner(
                    bincode::serde::encode_to_vec(&packet, bincode::config::standard())?,
                )))
            });

        Ok((Some(read), Some(write)))
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
        let (stream, addr) = self
            .socket
            .accept()
            .await
            .map_err(|_| RouteWeaverError::ConnectionFailed)?;

        let stream = tokio_tungstenite::accept_async_with_config(stream, Some(*WEB_SOCKET_CONFIG))
            .await
            .map_err(|_| RouteWeaverError::UnknownPacketDecoding)?;

        let (write, read) = stream.split();

        let read = read
            .map_err(|_| RouteWeaverError::UnknownPacketDecoding)
            .try_filter_map(|message| async move {
                match message {
                    Message::Binary(bytes) => {
                        let (packet, _) =
                            bincode::serde::decode_from_slice(&bytes, bincode::config::standard())?;

                        Ok(Some(packet))
                    }
                    Message::Close(_) => Ok(None),
                    _ => Err(RouteWeaverError::UnknownPacketDecoding),
                }
            });

        let write = write
            .sink_map_err(|_| RouteWeaverError::UnknownPacketDecoding)
            .with(move |packet| async move {
                Ok(Message::Binary(Bytes::from_owner(
                    bincode::serde::encode_to_vec(&packet, bincode::config::standard())?,
                )))
            });

        Ok(((Some(read), Some(write)), addr.into()))
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
