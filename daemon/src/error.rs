use routeweaver_common::RouteWeaverCommonError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RouteWeaverError {
    #[error("failed handshake")]
    FailedHandshake,
    #[error("io error: {0}")]
    Standard(#[from] std::io::Error),
    #[error("decoding error {0}")]
    MessagePackDecoding(#[from] bincode::error::DecodeError),
    #[error("encoding error {0}")]
    MessagePackEncoding(#[from] bincode::error::EncodeError),
    #[error("unknown packet decoding error")]
    UnknownPacketDecoding,
    #[error("unknown packet encoding error")]
    UnknownPacketEncoding,
    #[error("noise error: {0}")]
    SnowError(#[from] snow::Error),
    #[error("error from common: {0}")]
    RouteWeaverCommonError(#[from] RouteWeaverCommonError),
    #[error("invalid handshake message")]
    IncorrectHandshakeMessage,
    #[error("invalid transport message")]
    IncorrectTransportMessage,
    #[error("invalid address")]
    InvalidAddress,
    #[error("invalid protocol")]
    InvalidProtocol,
    #[error("invalid peer")]
    InvalidPeer,
    #[error("invalid port")]
    InvalidPort,
    #[error("invalid key")]
    InvalidKey,
    #[error("connection failed")]
    ConnectionFailed,
    #[error("invalid packet payload index {index}")]
    InvalidPacketPayloadIndex { index: u16 },
    #[error("missing head")]
    MissingHead,
    #[error("invalid client message")]
    InvalidClientMessage,
    #[error("config parsing error: {0}")]
    ConfigParsing(#[from] toml::de::Error),
}
