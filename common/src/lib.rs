use arrayvec::ArrayString;
use data_encoding::HEXLOWER_PERMISSIVE;
use error::Error;
use serde::{Deserialize, Serialize};
use std::{
    fmt::{Debug, Display},
    net::{IpAddr, SocketAddr, SocketAddrV4, SocketAddrV6},
    str::FromStr,
};
use thiserror::Error;
use url::{Host, Url};
use zeroize::Zeroize;

mod error;
pub use error::Error as RouteWeaverCommonError;

pub mod ipc;

/// Context id to identify a connection
///
/// Unique between two nodes
pub type ConnectionId = u32;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Address {
    /// IP
    Ip { address: IpAddr, port: u16 },
    /// Bluetooth
    Bluetooth { address: [u8; 6], psm: u16 },
}

impl From<SocketAddr> for Address {
    fn from(socket_addr: SocketAddr) -> Self {
        // Converts ipv4 mapped ipv6 addresses as well
        match socket_addr {
            SocketAddr::V4(ipv4) => ipv4.into(),
            SocketAddr::V6(ipv6) => ipv6.into(),
        }
    }
}

impl From<SocketAddrV4> for Address {
    fn from(socket_addr: SocketAddrV4) -> Self {
        Address::Ip {
            address: IpAddr::V4(*socket_addr.ip()),
            port: socket_addr.port(),
        }
    }
}

impl From<SocketAddrV6> for Address {
    fn from(socket_addr: SocketAddrV6) -> Self {
        if let Some(ipv4) = socket_addr.ip().to_ipv4_mapped() {
            return Address::Ip {
                address: IpAddr::V4(ipv4),
                port: socket_addr.port(),
            };
        }

        Address::Ip {
            address: IpAddr::V6(*socket_addr.ip()),
            port: socket_addr.port(),
        }
    }
}

impl TryFrom<Address> for SocketAddr {
    type Error = Error;

    fn try_from(value: Address) -> Result<Self, Self::Error> {
        match value {
            Address::Ip { address, port } => Ok(SocketAddr::new(address, port)),
            Address::Bluetooth { .. } => Err(Error::InvalidAddress),
        }
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Address::Ip { address, port } => write!(f, "{}:{}", address, port),
            Address::Bluetooth { address, psm } => write!(
                f,
                "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{}",
                address[0], address[1], address[2], address[3], address[4], address[5], psm
            ),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Zeroize)]
pub enum Protocol {
    Udp,
    Tcp,
    Ws,
    Wss,
    Bluetooth,
}

impl FromStr for Protocol {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_lowercase().as_str() {
            "tcp" => Ok(Protocol::Tcp),
            "udp" => Ok(Protocol::Udp),
            "ws" => Ok(Protocol::Ws),
            "wss" => Ok(Protocol::Wss),
            "bluetooth" => Ok(Protocol::Bluetooth),
            _ => Err(Error::InvalidProtocol),
        }
    }
}

impl Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Protocol::Udp => "udp",
                Protocol::Tcp => "tcp",
                Protocol::Ws => "ws",
                Protocol::Wss => "wss",
                Protocol::Bluetooth => "bluetooth",
            }
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Peer {
    pub protocol: Protocol,
    pub address: Address,
}

impl Peer {
    pub fn is_loopback(&self) -> bool {
        match self.address {
            Address::Ip { address, .. } => address.is_loopback(),
            Address::Bluetooth { .. } => false,
        }
    }
}

impl TryFrom<Url> for Peer {
    type Error = Error;

    fn try_from(value: Url) -> Result<Self, Self::Error> {
        Ok(Peer {
            protocol: value.scheme().parse()?,
            address: match value.host().ok_or(Error::InvalidAddress)? {
                Host::Domain(_) => todo!(),
                Host::Ipv4(ip) => Address::Ip {
                    address: IpAddr::V4(ip),
                    port: value.port_or_known_default().unwrap(),
                },
                Host::Ipv6(ip) => Address::Ip {
                    address: IpAddr::V6(ip),
                    port: value.port_or_known_default().unwrap(),
                },
            },
        })
    }
}

impl FromStr for Peer {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Format for peers is /protocol/address_family/address(? /optionals for addresses)

        let mut split = s.splitn(4, '/');
        let empty = split.next().ok_or(Error::InvalidPeer)?;
        let protocol = split.next().ok_or(Error::InvalidPeer)?;
        let address_family = split.next().ok_or(Error::InvalidPeer)?;
        let address = split.next().ok_or(Error::InvalidPeer)?;

        if !empty.is_empty() {
            return Err(Error::InvalidPeer);
        }

        Ok(Peer {
            protocol: protocol.parse()?,
            address: match address_family.to_ascii_lowercase().as_str() {
                "ip" => {
                    let mut split = address.splitn(2, '/');
                    let address = split
                        .next()
                        .ok_or(Error::InvalidAddress)?
                        .parse::<IpAddr>()
                        .map_err(|_| Error::InvalidAddress)?;
                    let port = split
                        .next()
                        .ok_or(Error::InvalidAddress)?
                        .parse()
                        .map_err(|_| Error::InvalidAddress)?;
                    Address::Ip { address, port }
                }
                "bluetooth" => {
                    let mut split = address.split('/');

                    let address_string = split.next().ok_or(Error::InvalidAddress)?;
                    let mut address_bytes = [0; 6];
                    for (i, byte) in address_string.splitn(6, ':').enumerate() {
                        address_bytes[i] =
                            u8::from_str_radix(byte, 16).map_err(|_| Error::InvalidAddress)?;
                    }

                    let psm = split
                        .next()
                        .ok_or(Error::InvalidAddress)?
                        .parse()
                        .map_err(|_| Error::InvalidAddress)?;
                    Address::Bluetooth {
                        address: address_bytes,
                        psm,
                    }
                }
                _ => return Err(Error::InvalidAddress),
            },
        })
    }
}

impl Display for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.address {
            Address::Ip { address, port } => {
                write!(f, "/{}/ip/{}/{}", self.protocol, address, port)
            }
            Address::Bluetooth { address, psm } => {
                write!(
                    f,
                    "/{}/bluetooth/{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}/{}",
                    self.protocol,
                    address[0],
                    address[1],
                    address[2],
                    address[3],
                    address[4],
                    address[5],
                    psm
                )
            }
        }
    }
}

#[derive(
    Serialize, Deserialize, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Zeroize, Error,
)]
pub struct PublicKey([u8; 32]);

impl PublicKey {
    pub const fn new(key: [u8; 32]) -> Self {
        PublicKey(key)
    }
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&HEXLOWER_PERMISSIVE.encode(&self.0))
    }
}

impl Debug for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&HEXLOWER_PERMISSIVE.encode(&self.0))
    }
}

impl FromStr for PublicKey {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(PublicKey(
            HEXLOWER_PERMISSIVE
                .decode(s.as_bytes())
                .map_err(|_| Error::InvalidKey)?
                .try_into()
                .map_err(|_| Error::InvalidKey)?,
        ))
    }
}

#[derive(
    Serialize, Deserialize, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Zeroize,
)]
pub struct ApplicationId(ArrayString<6>);

impl ApplicationId {
    pub fn new(id: &str) -> Self {
        ApplicationId(ArrayString::from(id).unwrap())
    }
}

impl AsRef<str> for ApplicationId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for ApplicationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ApplicationId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(ApplicationId(
            ArrayString::try_from(s).map_err(|_| Error::InvalidApplicationId)?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use url::Url;

    use crate::{Address, Peer, Protocol};

    #[test]
    fn parse_ipv4_peer() {
        let peer = "/tcp/ipv4/127.0.0.1/12345".parse::<Peer>().unwrap();
        assert_eq!(peer.protocol, Protocol::Tcp);
        assert_eq!(
            peer.address,
            Address::Ip {
                address: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                port: 12345
            }
        );
    }

    #[test]
    fn print_ipv4_peer() {
        let peer = Peer {
            protocol: Protocol::Tcp,
            address: Address::Ip {
                address: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                port: 12345,
            },
        };
        assert_eq!("/tcp/ipv4/127.0.0.1/12345", peer.to_string());
    }

    #[test]
    fn parse_ipv4_peer_from_url() {
        let peer: Peer = "ws://127.0.0.1:12345"
            .parse::<Url>()
            .unwrap()
            .try_into()
            .unwrap();

        assert_eq!(peer.protocol, Protocol::Ws);
        assert_eq!(
            peer.address,
            Address::Ip {
                address: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                port: 12345
            }
        );
    }

    #[test]
    fn parse_ipv6_peer() {
        let peer = "/tcp/ipv6/::1/12345".parse::<Peer>().unwrap();
        assert_eq!(peer.protocol, Protocol::Tcp);
        assert_eq!(
            peer.address,
            Address::Ip {
                address: IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                port: 12345
            }
        );
    }

    #[test]
    fn print_ipv6_peer() {
        let peer = Peer {
            protocol: Protocol::Tcp,
            address: Address::Ip {
                address: IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                port: 12345,
            },
        };
        assert_eq!("/tcp/ipv6/::1/12345", peer.to_string());
    }

    #[test]
    fn parse_ipv6_peer_from_url() {
        let peer: Peer = "wss://[::1]:12345"
            .parse::<Url>()
            .unwrap()
            .try_into()
            .unwrap();

        assert_eq!(peer.protocol, Protocol::Wss);
        assert_eq!(
            peer.address,
            Address::Ip {
                address: IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                port: 12345
            }
        );
    }

    #[test]
    fn parse_bluetooth_peer() {
        let peer = "/bluetooth/bluetooth/00:11:22:33:44:55/66"
            .parse::<Peer>()
            .unwrap();

        assert_eq!(peer.protocol, Protocol::Bluetooth);
        assert_eq!(
            peer.address,
            Address::Bluetooth {
                address: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
                psm: 66
            }
        );
    }

    #[test]
    fn print_bluetooth_peer() {
        let peer = Peer {
            protocol: Protocol::Bluetooth,
            address: Address::Bluetooth {
                address: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
                psm: 66,
            },
        };
        assert_eq!(
            "/bluetooth/bluetooth/00:11:22:33:44:55/66",
            peer.to_string()
        );
    }
}
