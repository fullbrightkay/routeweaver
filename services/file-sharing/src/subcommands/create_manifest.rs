use crate::manifest::{BlockId, Manifest, ManifestEntry, ManifestEntrySpecifics, BLOCK_SIZE};
use async_walkdir::WalkDir;
use blake2::Blake2s256;
use blake2::Digest;
use camino::Utf8PathBuf;
use chrono::DateTime;
use futures_util::TryStreamExt;
use std::{
    collections::HashMap,
    io::SeekFrom,
    path::{Path, PathBuf},
    time::SystemTime,
};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};

// TODO: Clean this up its scary
pub async fn create_manifest(manifest_path: PathBuf, paths: Vec<PathBuf>) {
    let mut manifest = Manifest::new(None);

    for base_path in paths {
        if base_path.is_file() {
            let (hash, blocks) = split_file(&base_path).await;
            let entry_name = base_path.file_name().unwrap().to_str().unwrap().to_string();
            let metadata = tokio::fs::metadata(&base_path).await.unwrap();

            manifest.root.insert(
                entry_name,
                ManifestEntry {
                    creation_date: Some(DateTime::from(metadata.created().unwrap())),
                    modified_date: Some(DateTime::from(metadata.modified().unwrap())),
                    specifics: ManifestEntrySpecifics::File { hash, blocks },
                },
            );
        } else if base_path.is_dir() {
            let mut walker = WalkDir::new(&base_path);

            while let Ok(Some(entry)) = walker.try_next().await {
                let path = entry.path();
                let stripped_path = path.strip_prefix(&base_path).unwrap();
                let metadata = tokio::fs::metadata(&path).await.unwrap();

                let specifics = if path.is_file() {
                    let (hash, blocks) = split_file(&path).await;

                    ManifestEntrySpecifics::File { hash, blocks }
                } else if path.is_dir() {
                    ManifestEntrySpecifics::Directory(HashMap::default())
                } else {
                    unreachable!()
                };

                manifest
                    .create(
                        Utf8PathBuf::from_path_buf(stripped_path.to_owned()).unwrap(),
                        ManifestEntry {
                            creation_date: Some(DateTime::from(metadata.created().unwrap())),
                            modified_date: Some(DateTime::from(metadata.modified().unwrap())),
                            specifics,
                        },
                    )
                    .unwrap();
            }
        }
    }

    tracing::info!("Created manifest");

    let mut file = File::create(manifest_path).await.unwrap();
    let mut buffer = Vec::new();
    bincode::serde::encode_into_std_write(manifest, &mut buffer, bincode::config::standard())
        .unwrap();
    file.write_all(&buffer).await.unwrap();
}

async fn split_file(path: impl AsRef<Path>) -> ([u8; 32], Vec<BlockId>) {
    let mut file = File::open(path).await.unwrap();
    let mut blocks = Vec::default();
    let total_hasher = Blake2s256::default();
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

        blocks.push(hash);
        cursor += amount as u64;
    }

    (total_hasher.finalize().into(), blocks)
}
