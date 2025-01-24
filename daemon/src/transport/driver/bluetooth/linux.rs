use super::BluetoothTransportConfig;
use crate::{
    error::RouteWeaverError,
    proto::{Packet, MAX_SERIALIZED_PACKET_SIZE},
    transport::{Transport, TransportReader, TransportWriter},
};
use bincode::serde::{decode_from_slice, encode_into_slice};
use bluer::l2cap::{SeqPacket, SeqPacketListener, SocketAddr, PSM_LE_DYN_START};
use routeweaver_common::{Address, Peer, Protocol, BASE_BINCODE_CONFIG};
use std::sync::Arc;

pub struct BluetoothTransport {
    session: bluer::Session,
    socket: SeqPacketListener,
    listen_psm: u16,
}

impl BluetoothTransport {
    pub async fn new(config: BluetoothTransportConfig) -> Result<Self, RouteWeaverError> {
        let listen_psm = config.listen_psm.unwrap_or(PSM_LE_DYN_START);

        Ok(Self {
            session: bluer::Session::new().await.unwrap(),
            socket: SeqPacketListener::bind(SocketAddr::new(
                bluer::Address::any(),
                bluer::AddressType::LePublic,
                listen_psm,
            ))
            .await?,
            listen_psm,
        })
    }
}

impl Transport for BluetoothTransport {
    const PROTOCOL: Protocol = Protocol::Bluetooth;

    async fn connect(
        &self,
        peer: &Peer,
    ) -> Result<(impl TransportReader, impl TransportWriter), RouteWeaverError> {
        assert!(peer.protocol == Protocol::Bluetooth);

        let Address::Bluetooth { address, psm } = peer.address else {
            unreachable!()
        };

        let address = SocketAddr::new(bluer::Address(address), bluer::AddressType::LePublic, psm);

        Ok(SeqPacket::connect(address).await.map(|connection| {
            let connection = Arc::new(connection);

            (
                {
                    let connection = connection.clone();
                    let buffer = vec![0; MAX_SERIALIZED_PACKET_SIZE];

                    futures_util::stream::try_unfold((connection, buffer), connection_reader)
                },
                {
                    let connection = connection.clone();
                    let buffer = vec![0; MAX_SERIALIZED_PACKET_SIZE];

                    futures_util::sink::unfold((connection, buffer), connection_writer)
                },
            )
        })?)
    }

    async fn accept(
        &self,
    ) -> Result<((impl TransportReader, impl TransportWriter), Peer), RouteWeaverError> {
        let (connection, address) = self
            .socket
            .accept()
            .await
            .map_err(|_| RouteWeaverError::ConnectionFailed)?;
        let connection = Arc::new(connection);

        Ok((
            (
                {
                    let buffer = vec![0; MAX_SERIALIZED_PACKET_SIZE];

                    futures_util::stream::try_unfold(
                        (connection.clone(), buffer),
                        connection_reader,
                    )
                },
                {
                    let buffer = vec![0; MAX_SERIALIZED_PACKET_SIZE];

                    futures_util::sink::unfold((connection.clone(), buffer), connection_writer)
                },
            ),
            Peer {
                protocol: Protocol::Bluetooth,
                address: Address::Bluetooth {
                    address: address.addr.0,
                    psm: address.psm,
                },
            },
        ))
    }

    async fn local_addrs(&self) -> Result<impl Iterator<Item = Peer>, RouteWeaverError> {
        let mut addresses = Vec::new();

        for adapter in self.session.adapter_names().await.unwrap() {
            if let Ok(adapter) = self.session.adapter(&adapter) {
                addresses.push(
                    adapter
                        .address()
                        .await
                        .map(|address| Peer {
                            protocol: Protocol::Bluetooth,
                            address: Address::Bluetooth {
                                address: address.0,
                                psm: self.listen_psm,
                            },
                        })
                        .map_err(|_| RouteWeaverError::ConnectionFailed)?,
                );
            }
        }

        Ok(addresses.into_iter())
    }
}

async fn connection_reader(
    state: (Arc<SeqPacket>, Vec<u8>),
) -> Result<Option<(Packet, (Arc<SeqPacket>, Vec<u8>))>, RouteWeaverError> {
    let (connection, mut buffer) = state;

    let amount = connection
        .recv(&mut buffer)
        .await
        .map_err(|_| RouteWeaverError::ConnectionFailed)?;

    if amount == 0 {
        return Ok(None);
    }

    let (packet, _) = decode_from_slice(
        &buffer[..amount],
        BASE_BINCODE_CONFIG.with_limit::<MAX_SERIALIZED_PACKET_SIZE>(),
    )?;

    Ok(Some((packet, (connection, buffer))))
}

async fn connection_writer(
    state: (Arc<SeqPacket>, Vec<u8>),
    packet: Packet,
) -> Result<(Arc<SeqPacket>, Vec<u8>), RouteWeaverError> {
    let (connection, mut buffer) = state;
    let amount = encode_into_slice(
        packet,
        &mut buffer,
        BASE_BINCODE_CONFIG.with_limit::<MAX_SERIALIZED_PACKET_SIZE>(),
    )?;

    connection
        .send(&buffer[..amount])
        .await
        .map_err(|_| RouteWeaverError::ConnectionFailed)?;

    Ok((connection, buffer))
}
