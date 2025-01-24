use crate::{error::Error, ApplicationId, PublicKey};
use bincode::error::DecodeError;
use futures_util::{SinkExt, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::net::UnixStream;
use tokio_util::{
    bytes::{Buf, BufMut, BytesMut},
    codec::{Decoder, Encoder, Framed},
};

use super::{stream::RouteWeaverStream, StreamAuthToken};

struct ConnectionParser;

impl Encoder<ServerBoundSocketIpc> for ConnectionParser {
    type Error = Error;

    fn encode(
        &mut self,
        item: ServerBoundSocketIpc,
        dst: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        bincode::serde::encode_into_std_write(
            &item,
            &mut dst.writer(),
            bincode::config::standard(),
        )?;
        Ok(())
    }
}

impl Decoder for ConnectionParser {
    type Item = ClientBoundSocketIpc;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            return Ok(None);
        }

        match bincode::serde::decode_from_std_read(&mut src.reader(), bincode::config::standard()) {
            Ok(item) => Ok(Some(item)),
            Err(DecodeError::UnexpectedEnd { additional }) => {
                src.reserve(additional);
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ServerBoundSocketIpc {
    Listen {
        application_id: ApplicationId,
    },
    Connect {
        application_id: ApplicationId,
        destination: PublicKey,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ClientBoundSocketIpc {
    Success,
    Stream {
        stream_socket_path: PathBuf,
        stream_auth_token: StreamAuthToken,
    },
}

pub struct RouteWeaverSocket {
    ipc_connection: Framed<UnixStream, ConnectionParser>,
}

impl RouteWeaverSocket {
    pub(crate) async fn connect(
        ipc_server_path: impl AsRef<Path>,
        node: PublicKey,
        application: ApplicationId,
    ) -> Result<RouteWeaverStream, Error> {
        let stream = UnixStream::connect(&ipc_server_path).await?;
        let mut ipc_connection = Framed::new(stream, ConnectionParser);

        ipc_connection
            .send(ServerBoundSocketIpc::Connect {
                application_id: application,
                destination: node,
            })
            .await
            .unwrap();

        match ipc_connection.next().await {
            Some(Ok(ClientBoundSocketIpc::Success)) => {}
            Some(Ok(_)) => return Err(Error::UnexpectedIpcServerMessage),
            Some(Err(e)) => return Err(e),
            None => return Err(Error::UnexpectedIpcServerConnectionClose),
        }

        match ipc_connection.next().await {
            // If we actually got the message
            Some(Ok(ClientBoundSocketIpc::Stream {
                stream_socket_path,
                stream_auth_token,
            })) => Ok(RouteWeaverStream::new(stream_socket_path, stream_auth_token).await?),
            // If we got some other message
            Some(Ok(_)) => Err(Error::UnexpectedIpcServerMessage),
            Some(Err(err)) => Err(err),
            None => Err(Error::UnexpectedIpcServerConnectionClose),
        }
    }

    pub async fn bind(
        ipc_server_path: impl AsRef<Path>,
        application: ApplicationId,
    ) -> Result<Self, Error> {
        let stream = UnixStream::connect(&ipc_server_path).await?;
        let mut ipc_connection = Framed::new(stream, ConnectionParser);

        ipc_connection
            .send(ServerBoundSocketIpc::Listen {
                application_id: application,
            })
            .await
            .unwrap();

        match ipc_connection.next().await {
            Some(Ok(ClientBoundSocketIpc::Success)) => {}
            Some(Ok(_)) => return Err(Error::UnexpectedIpcServerMessage),
            Some(Err(e)) => return Err(e),
            None => return Err(Error::UnexpectedIpcServerConnectionClose),
        }

        Ok(Self { ipc_connection })
    }
}

impl Stream for RouteWeaverSocket {
    type Item = Result<(PublicKey, RouteWeaverStream), Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        todo!()
    }
}
