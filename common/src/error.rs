#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error: {0}")]
    StandardError(#[from] std::io::Error),
    #[error("Invalid address")]
    InvalidAddress,
    #[error("Invalid key")]
    InvalidKey,
    #[error("Invalid message")]
    InvalidMessage,
    #[error("Invalid packet")]
    InvalidPacket,
    #[error("Invalid protocol")]
    InvalidProtocol,
    #[error("Invalid peer")]
    InvalidPeer,
    #[error("Invalid port")]
    InvalidPort,
    #[error("Invalid application id")]
    InvalidApplicationId,
    #[error("Unexpected ipc server connection close")]
    UnexpectedIpcServerConnectionClose,
    #[error("Unexpected ipc server message")]
    UnexpectedIpcServerMessage,
    #[error("Bincode encoding error: {0}")]
    BincodeEncoding(#[from] bincode::error::EncodeError),
    #[error("Bincode decoding error: {0}")]
    BincodeDecoding(#[from] bincode::error::DecodeError),
}
