use crate::{database::BlockHash, manifest::Manifest, subcommands::Message, BLOCK_SIZE};
use arrayvec::ArrayVec;
use blake2::Blake2s256;
use blake2::Digest;
use dashmap::{DashMap, DashSet};
use futures_util::{future::join_all, SinkExt, TryStreamExt};
use rand::{
    prelude::{IteratorRandom, StdRng},
    SeedableRng,
};
use routeweaver_common::{
    message::{
        ClientBoundIpc, RoleSetSuccessSpecifics, ServerBoundIpc, ServerConnectionParser,
        SetRoleSpecifics,
    },
    ConnectionId, PublicKey,
};
use std::{collections::HashSet, path::Path, sync::Arc};
use tokio::{
    fs::File,
    io::AsyncWriteExt,
    net::{unix::OwnedWriteHalf, UnixStream},
    sync::{mpsc, Notify},
};
use tokio_util::codec::{Framed, FramedRead, FramedWrite};

pub type BlockTracker = DashMap<BlockHash, ArrayVec<u8, BLOCK_SIZE>>;

pub async fn download(manifest: impl AsRef<Path>, file_save_location: impl AsRef<Path>) {
    let mut output_file = File::create(file_save_location).await.unwrap();
    let manifest = tokio::fs::read(manifest).await.unwrap();
    let manifest: Manifest = rmp_serde::from_slice(&manifest).unwrap();

    // Fetch the local address
    let Ok(Some(ClientBoundIpc::NodeInfo { id })) = Framed::new(
        UnixStream::connect("sockets/daemon").await.unwrap(),
        ServerConnectionParser,
    )
    .try_next()
    .await
    else {
        tracing::error!("Server should have sent a NodeInfo message first");
        return;
    };

    let mut needed_blocks = manifest
        .block_hashes
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let block_tracker = Arc::new(BlockTracker::default());

    let nodes_to_contact = Arc::new(DashSet::from_iter(std::iter::once(id)));

    tracing::info!(
        "Loading manifest representing file of hash {} and with {} blocks",
        data_encoding::HEXLOWER_PERMISSIVE.encode(&manifest.representing_hash),
        manifest.block_hashes.len()
    );

    loop {
        let mut spawned_tasks = Vec::new();

        for (node, block_chunk) in nodes_to_contact.iter().take(num_cpus::get()).zip(
            needed_blocks
                .iter()
                .copied()
                .collect::<Vec<_>>()
                .chunks(needed_blocks.len() / nodes_to_contact.len())
                .map(|chunk| HashSet::from_iter(chunk.iter().copied())),
        ) {
            spawned_tasks.push(tokio::spawn(gather_from_node(
                *node,
                block_tracker.clone(),
                block_chunk,
                nodes_to_contact.clone(),
            )));
        }

        join_all(spawned_tasks).await;

        // Remove all blocks we have already gathered
        for block in block_tracker.iter() {
            needed_blocks.remove(block.key());
        }

        // Check if we have everything required to construct the file
        if needed_blocks.is_empty() {
            break;
        }
    }

    let mut hasher = Blake2s256::default();

    for block in &manifest.block_hashes {
        let block_data = block_tracker.get(block).unwrap();
        hasher.update(block_data.as_slice());
    }

    let representing_hash: [u8; 32] = hasher.finalize().into();
    if manifest.representing_hash != representing_hash {
        tracing::error!("Hash mismatch for manifest");
        return;
    }

    for block in &manifest.block_hashes {
        let block_data = block_tracker.get(block).unwrap();
        output_file.write_all(block_data.as_slice()).await.unwrap();
    }
}

pub async fn gather_from_node(
    node: PublicKey,
    block_tracker: Arc<BlockTracker>,
    needed_blocks: HashSet<BlockHash>,
    nodes_to_contact: Arc<DashSet<PublicKey>>,
) {
    let application_id = "fs".parse().unwrap();

    tracing::debug!("Gathering blocks from {}", node);

    let stream = UnixStream::connect("sockets/daemon").await.unwrap();

    let (connection_reader, connection_writer) = stream.into_split();
    let (mut connection_reader, mut connection_writer) = (
        FramedRead::new(connection_reader, ServerConnectionParser),
        FramedWrite::new(connection_writer, ServerConnectionParser),
    );

    let Ok(Some(ClientBoundIpc::NodeInfo { id })) = connection_reader.try_next().await else {
        tracing::error!("Server should have sent a NodeInfo message first");
        return;
    };

    tracing::info!("Connected to server with node id {}", id);

    if connection_writer
        .send(ServerBoundIpc::SetRole {
            specifics: SetRoleSpecifics::Connect {
                application: application_id,
                destination: node,
            },
        })
        .await
        .is_err()
    {
        tracing::error!("Failed to send SetRole message to server");
        return;
    }

    let Ok(Some(ClientBoundIpc::RoleSetSuccess {
        specifics:
            RoleSetSuccessSpecifics::Connect {
                connection_id: node_connection_id,
            },
    })) = connection_reader.try_next().await
    else {
        tracing::error!("Failed to confirm role set");
        return;
    };

    let message_confirmed_notify = Arc::new(Notify::new());
    let (message_channel_sender, message_channel_receiver) = mpsc::unbounded_channel();
    let connection_handler = tokio::spawn(connection_handler(
        node_connection_id,
        message_confirmed_notify.clone(),
        block_tracker,
        needed_blocks,
        message_channel_receiver,
        connection_writer,
        nodes_to_contact,
    ));

    while let Ok(Some(message)) = connection_reader.try_next().await {
        match message {
            ClientBoundIpc::Data {
                connection_id,
                data,
            } => {
                debug_assert_eq!(node_connection_id, connection_id);

                let Ok(message) = rmp_serde::from_slice(&data) else {
                    tracing::error!("Failed to deserialize message");
                    return;
                };

                if message_channel_sender.send(message).is_err() {
                    return;
                }
            }
            ClientBoundIpc::DataSuccess { connection_id } => {
                debug_assert_eq!(node_connection_id, connection_id);

                message_confirmed_notify.notify_one();
            }
            ClientBoundIpc::ConnectionClosed { connection_id } => {
                debug_assert_eq!(node_connection_id, connection_id);

                connection_handler.abort();
                tracing::debug!("Connection to node {} closed", node);
                return;
            }
            _ => {}
        }
    }
}

pub async fn connection_handler(
    connection_id: ConnectionId,
    message_confirm_notify: Arc<Notify>,
    block_tracker: Arc<BlockTracker>,
    mut needed_blocks: HashSet<BlockHash>,
    mut message_channel: mpsc::UnboundedReceiver<Message>,
    mut connection_writer: FramedWrite<OwnedWriteHalf, ServerConnectionParser>,
    nodes_to_contact: Arc<DashSet<PublicKey>>,
) {
    let mut rng = StdRng::from_entropy();

    while let Some(message) = message_channel.recv().await {
        match message {
            Message::Block { block, data } => {
                if let dashmap::Entry::Vacant(entry) = block_tracker.entry(block) {
                    let mut hasher = Blake2s256::default();
                    hasher.update(data.as_slice());
                    let hash = BlockHash::new(hasher.finalize().into());

                    if hash == block {
                        needed_blocks.remove(&block);
                        entry.insert(*data);
                    } else {
                        tracing::error!("Hash mismatch for block");
                        return;
                    }
                }

                let Some(next_block) = needed_blocks
                    .iter()
                    .filter(|block| !block_tracker.contains_key(block))
                    .choose(&mut rng)
                    .copied()
                else {
                    return;
                };

                connection_writer
                    .send(ServerBoundIpc::Data {
                        connection_id,
                        data: rmp_serde::encode::to_vec_named(&Message::QueryBlock {
                            block: next_block,
                        })
                        .unwrap(),
                    })
                    .await
                    .unwrap();

                message_confirm_notify.notified().await;
            }
            Message::BlockElsewhere { block, location } => {
                nodes_to_contact.insert(location);
            }
            Message::UnknownBlock { block } => todo!(),
            _ => {}
        }
    }
}
