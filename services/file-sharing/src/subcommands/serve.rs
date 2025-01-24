use blake2::Digest;
use futures_util::{SinkExt, TryStreamExt};
use routeweaver_common::{
    ipc::{socket::RouteWeaverSocket, stream::RouteWeaverStream, DAEMON_RPC_SOCKET},
    PublicKey,
};
use std::{ops::Deref, path::PathBuf, sync::Arc, time::Duration};
use tokio::time::timeout;

use crate::{client::Client, APPLICATION_ID};

use super::Message;

pub async fn serve(files: Vec<PathBuf>, max_cache_size: u64) {
    let mut socket = RouteWeaverSocket::bind(DAEMON_RPC_SOCKET.deref(), *APPLICATION_ID)
        .await
        .unwrap();
    let client = Arc::new(Client::new(max_cache_size));
    client.add_local_files(files).await;

    while let Some((origin, stream)) = socket.try_next().await.unwrap() {
        let client = client.clone();

        tokio::spawn(async move {
            serve_files(client, origin, stream).await;
        });
    }
}

pub async fn serve_files(client: Arc<Client>, origin: PublicKey, mut stream: RouteWeaverStream) {
    while let Ok(Some(message)) = stream.try_next().await {
        if let Ok((decoded_message, _)) =
            bincode::serde::decode_from_slice::<Message, _>(&message, bincode::config::standard())
        {
            match decoded_message {
                Message::QueryBlock { block } => {
                    if let Ok(data) =
                        timeout(Duration::from_secs(5), client.query_block(block)).await
                    {
                        tracing::debug!(
                            "Node {} requested block {} and we returned it successfully",
                            origin,
                            block,
                        );

                        stream
                            .send(
                                bincode::serde::encode_to_vec(
                                    Message::Block { block, data },
                                    bincode::config::standard(),
                                )
                                .unwrap(),
                            )
                            .await;
                    } else {
                        tracing::debug!(
                            "Node {} requested block {} however we could not find it",
                            origin,
                            block
                        );

                        stream
                            .send(
                                bincode::serde::encode_to_vec(
                                    Message::UnknownBlock { block },
                                    bincode::config::standard(),
                                )
                                .unwrap(),
                            )
                            .await;
                    }
                }
                Message::Block { block, data } => {
                    client.add_block(data).await;
                }
                Message::BlockElsewhere { block, location } => todo!(),
                Message::UnknownBlock { block } => todo!(),
            }
        }
    }
}
