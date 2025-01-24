use crate::manifest::{BlockId, BLOCK_SIZE};
use async_walkdir::WalkDir;
use blake2::Blake2s256;
use blake2::Digest;
use futures_util::StreamExt;
use moka::future::Cache;
use rangemap::RangeMap;
use std::{
    io::SeekFrom,
    ops::Range,
    path::{Path, PathBuf},
    sync::LazyLock,
};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt},
};

static BLOCK_STORAGE: LazyLock<PathBuf> = LazyLock::new(|| {
    dirs::data_local_dir()
        .unwrap()
        .join("routeweaver")
        .join("service")
        .join("file-sharing")
        .join("blocks")
});

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum BlockLocation {
    ExternalFile { path: PathBuf, range: Range<u64> },
    Internal,
}

pub struct Client {
    block_cache: Cache<BlockId, BlockLocation>,
}

impl Client {
    pub fn new(max_cache_size: u64) -> Self {
        todo!()
    }

    pub async fn add_local_files(&self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths.into_iter() {
            if path.is_file() {
                for (range, hash) in split_file(&path).await {
                    self.block_cache
                        .insert(
                            hash,
                            BlockLocation::ExternalFile {
                                path: path.clone(),
                                range,
                            },
                        )
                        .await;
                }
            } else if path.is_dir() {
                let mut walkdir = WalkDir::new(path);
                while let Some(Ok(entry)) = walkdir.next().await {
                    let path = entry.path();

                    if path.is_file() {
                        for (range, hash) in split_file(&path).await {
                            self.block_cache
                                .insert(
                                    hash,
                                    BlockLocation::ExternalFile {
                                        path: path.clone(),
                                        range,
                                    },
                                )
                                .await;
                        }
                    }
                }
            } else {
                tracing::debug!("Skipping non-file/directory {}", path.display());
            }
        }
    }

    pub async fn add_block(&self, block: Vec<u8>) {}

    pub async fn query_block(&self, block: impl Into<BlockId>) -> Vec<u8> {
        let block = block.into();
        let mut buffer = Vec::with_capacity(BLOCK_SIZE);

        match self.block_cache.get(&block).await {
            // Is part of some file system file
            Some(BlockLocation::ExternalFile { path, range }) => {
                let mut file = File::open(path).await.unwrap();

                file.seek(SeekFrom::Start(range.start)).await.unwrap();
                file.take(range.end - range.start)
                    .read_to_end(&mut buffer)
                    .await
                    .unwrap();
            }
            // Is part of the internal block storage
            Some(BlockLocation::Internal) => {
                todo!()
            }
            // We need to search for it
            None => {
                todo!()
            }
        }

        buffer
    }
}

async fn split_file(path: impl AsRef<Path>) -> RangeMap<u64, BlockId> {
    let mut file = File::open(path).await.unwrap();
    let mut blocks = RangeMap::new();
    let mut read_buffer = Vec::with_capacity(BLOCK_SIZE);

    let mut cursor = 0;

    loop {
        read_buffer.clear();
        file.seek(SeekFrom::Start(cursor)).await.unwrap();

        let amount = (&mut file)
            .take(BLOCK_SIZE as u64)
            .read_to_end(&mut read_buffer)
            .await
            .unwrap();

        if amount == 0 {
            break;
        }

        let mut hasher = Blake2s256::default();
        hasher.update(&read_buffer[..amount]);
        let hash = BlockId::new(hasher.finalize().into());

        blocks.insert(cursor..(cursor + amount as u64), hash);
        cursor += amount as u64;
    }

    blocks
}
