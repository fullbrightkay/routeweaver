#[cfg(transport_bluetooth)]
pub mod bluetooth;
#[cfg(transport_tcp)]
pub mod tcp;
#[cfg(transport_ws)]
pub mod ws;
#[cfg(transport_wss)]
pub mod wss;

use crate::error::RouteWeaverError;
use crate::transport::packet::Packet;
use bytes::BytesMut;
use futures_util::{Sink, Stream};
use routeweaver_common::{Address, Peer, Protocol};
use sealed::sealed;
use std::{fmt::Debug, future::Future};
use tokio_util::{
    bytes::{Buf, BufMut},
    codec::{Decoder, Encoder},
};

#[sealed]
pub trait TransportReader:
    Send + Sync + Stream<Item = Result<Packet, RouteWeaverError>> + 'static
{
}
#[sealed]
impl<T: Send + Sync + Stream<Item = Result<Packet, RouteWeaverError>> + 'static> TransportReader
    for T
{
}

#[sealed]
pub trait TransportWriter: Sink<Packet, Error = RouteWeaverError> + Send + Sync + 'static {}
#[sealed]
impl<T: Sink<Packet, Error = RouteWeaverError> + Send + Sync + 'static> TransportWriter for T {}

pub trait Transport: Sized + Send + Sync + 'static {
    const PROTOCOL: Protocol;

    async fn from_config(config: toml::Value) -> Result<Self, RouteWeaverError>;

    fn connect(
        &self,
        address: &Address,
    ) -> impl Future<
        Output = Result<
            (Option<impl TransportReader>, Option<impl TransportWriter>),
            RouteWeaverError,
        >,
    > + Send;

    fn accept(
        &self,
    ) -> impl Future<
        Output = Result<
            (
                (Option<impl TransportReader>, Option<impl TransportWriter>),
                Address,
            ),
            RouteWeaverError,
        >,
    > + Send;

    fn local_addresses(
        &self,
    ) -> impl Future<Output = Result<impl Iterator<Item = Address> + Send, RouteWeaverError>> + Send;
}

#[derive(Default, Debug)]
pub struct PacketEncoderDecoder;

impl Decoder for PacketEncoderDecoder {
    type Item = Packet;
    type Error = RouteWeaverError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            return Ok(None);
        }

        // On purpose our packets don't have any magic bytes

        match bincode::serde::decode_from_std_read(&mut src.reader(), bincode::config::standard()) {
            Ok(packet) => Ok(Some(packet)),
            Err(bincode_error) => {
                match &bincode_error {
                    // Return Ok(None) if its a unexpected EOF
                    bincode::error::DecodeError::UnexpectedEnd { .. } => Ok(None),
                    _ => Err(bincode_error.into()),
                }
            }
        }
    }
}

impl Encoder<Packet> for PacketEncoderDecoder {
    type Error = RouteWeaverError;

    fn encode(&mut self, item: Packet, dst: &mut BytesMut) -> Result<(), Self::Error> {
        bincode::serde::encode_into_std_write(
            &item,
            &mut dst.writer(),
            bincode::config::standard(),
        )?;

        Ok(())
    }
}
