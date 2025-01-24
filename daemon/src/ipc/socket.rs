use crate::error::RouteWeaverError;
use bincode::error::DecodeError;
use bytes::{Buf, BufMut, BytesMut};
use futures_util::StreamExt;
use routeweaver_common::{
    ipc::{
        socket::{ClientBoundSocketIpc, ServerBoundSocketIpc},
        DAEMON_RPC_SOCKET,
    },
    ApplicationId, ConnectionId,
};
use std::{ops::Deref, sync::Arc};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::mpsc,
};
use tokio_util::codec::{Decoder, Encoder, Framed};

struct ConnectionParser;

impl Encoder<ClientBoundSocketIpc> for ConnectionParser {
    type Error = RouteWeaverError;

    fn encode(
        &mut self,
        item: ClientBoundSocketIpc,
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
    type Item = ServerBoundSocketIpc;
    type Error = RouteWeaverError;

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

pub struct NewConnection {
    pub id: ConnectionId,
    pub receiver: mpsc::Receiver<Vec<u8>>,
}

#[derive(Debug, Default)]
pub struct ApplicationTracker(scc::HashMap<ApplicationId, mpsc::Sender<NewConnection>>);

pub async fn socket_handler(application_tracker: Arc<ApplicationTracker>) {
    let ipc_socket = UnixListener::bind(DAEMON_RPC_SOCKET.deref()).unwrap();

    loop {
        match ipc_socket.accept().await {
            Ok((stream, _)) => {
                tracing::debug!("Accepted connection");

                let ipc_connection = Framed::new(stream, ConnectionParser);
                tokio::spawn(connection_handler(
                    application_tracker.clone(),
                    ipc_connection,
                ));
            }
            Err(err) => {
                tracing::error!("Error accepting connection: {}", err);
            }
        }
    }
}

async fn connection_handler(
    application_tracker: Arc<ApplicationTracker>,
    mut ipc_connection: Framed<UnixStream, ConnectionParser>,
) {
    while let Some(Ok(message)) = ipc_connection.next().await {
        match message {
            ServerBoundSocketIpc::Listen { application_id } => {
                let (sender, receiver) = mpsc::channel(100);

                if application_tracker
                    .0
                    .insert_async(application_id, sender)
                    .await
                    .is_err()
                {
                    tracing::error!(
                        "Service tried to listen while another service is listening on {}",
                        application_id
                    );
                } else {
                    tokio::spawn(connection_listener(
                        application_tracker.clone(),
                        receiver,
                        application_id,
                        false,
                    ));
                }
            }
            ServerBoundSocketIpc::Connect {
                application_id,
                destination,
            } => todo!(),
        }
    }
}

async fn connection_listener(
    application_tracker: Arc<ApplicationTracker>,
    mut new_connections: mpsc::Receiver<NewConnection>,
    application_id: ApplicationId,
    single: bool,
) {
    application_tracker.0.remove_async(&application_id).await;

    while let Some(NewConnection { id, receiver }) = new_connections.recv().await {}
}
