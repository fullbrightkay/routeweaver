use super::{socket::RouteWeaverSocket, StreamAuthToken};
use crate::{error::Error, ApplicationId, PublicKey};
use bincode::error::DecodeError;
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::net::UnixStream;
use tokio_util::{
    bytes::{Buf, BufMut, BytesMut},
    codec::{Decoder, Encoder, Framed},
};

#[derive(Debug, Serialize, Deserialize)]
pub enum ServerBoundStreamIpc {
    Auth { token: StreamAuthToken },
    Data { data: Vec<u8> },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ClientBoundStreamIpc {
    AuthSuccess,
    DataSendingSuccess,
}

struct ConnectionParser;

impl Encoder<ServerBoundStreamIpc> for ConnectionParser {
    type Error = Error;

    fn encode(
        &mut self,
        item: ServerBoundStreamIpc,
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
    type Item = ClientBoundStreamIpc;
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

pub struct RouteWeaverStream {
    ipc_connection: Framed<UnixStream, ConnectionParser>,
}

impl RouteWeaverStream {
    pub async fn connect(
        stream_socket_path: impl AsRef<Path>,
        node: PublicKey,
        application: ApplicationId,
    ) -> Result<Self, Error> {
        RouteWeaverSocket::connect(stream_socket_path, node, application).await
    }

    pub(crate) async fn new(
        stream_socket_path: impl AsRef<Path>,
        stream_auth_token: StreamAuthToken,
    ) -> Result<Self, Error> {
        let mut ipc_connection = Framed::new(
            UnixStream::connect(stream_socket_path).await?,
            ConnectionParser,
        );

        ipc_connection
            .send(ServerBoundStreamIpc::Auth {
                token: stream_auth_token,
            })
            .await?;

        match ipc_connection.next().await {
            Some(Ok(ClientBoundStreamIpc::AuthSuccess)) => {}
            Some(Ok(_)) => return Err(Error::UnexpectedIpcServerMessage),
            Some(Err(err)) => return Err(err),
            None => return Err(Error::UnexpectedIpcServerConnectionClose),
        }

        Ok(Self { ipc_connection })
    }
}

impl Stream for RouteWeaverStream {
    type Item = Result<Vec<u8>, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        todo!()
    }
}

impl Sink<Vec<u8>> for RouteWeaverStream {
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.ipc_connection.poll_ready_unpin(cx).is_ready() {
            return Poll::Ready(Ok(()));
        }

        Poll::Pending
    }

    fn start_send(mut self: Pin<&mut Self>, item: Vec<u8>) -> Result<(), Self::Error> {
        let message = ServerBoundStreamIpc::Data { data: item };

        self.ipc_connection
            .start_send_unpin(message)
            .map_err(Error::from)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.ipc_connection
            .poll_flush_unpin(cx)
            .map_err(Error::from)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.ipc_connection
            .poll_close_unpin(cx)
            .map_err(Error::from)
    }
}
