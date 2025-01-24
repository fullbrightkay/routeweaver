use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use data_encoding::HEXLOWER_PERMISSIVE;
use routeweaver_common::PublicKey;
use serde::{Deserialize, Serialize};
use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    fmt::Display,
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ManifestError {
    #[error("Failed to deserialize this manifest: {0}")]
    Deserialization(#[from] bincode::error::DecodeError),
    #[error("Failed to serialize this manifest: {0}")]
    Serialization(#[from] bincode::error::EncodeError),
    #[error("Failed to create entry in manifest: {0}")]
    Create(Utf8PathBuf),
}

pub const BLOCK_SIZE: usize = 128 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId([u8; 32]);

impl BlockId {
    pub fn new(hash: [u8; 32]) -> Self {
        Self(hash)
    }
}

impl<T: Into<[u8; 32]>> From<T> for BlockId {
    fn from(hash: T) -> Self {
        Self(hash.into())
    }
}

impl Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", HEXLOWER_PERMISSIVE.encode(&self.0))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

#[derive(Serialize, Deserialize)]
pub struct Manifest {
    /// Who made this manifest
    pub author: Option<PublicKey>,
    /// The root of the vfs
    pub root: HashMap<String, ManifestEntry>,
}

impl Manifest {
    pub fn new(author: Option<PublicKey>) -> Self {
        Manifest {
            author,
            root: HashMap::default(),
        }
    }

    pub fn create(
        &mut self,
        path: impl AsRef<Utf8Path>,
        entry: ManifestEntry,
    ) -> Result<(), ManifestError> {
        let path = path.as_ref();

        match &entry.specifics {
            ManifestEntrySpecifics::Directory(contents) => {
                tracing::info!(
                    "Adding directory {} to manifest with {} internal entries",
                    path,
                    contents.len()
                )
            }
            ManifestEntrySpecifics::File { hash: _, blocks } => {
                tracing::info!(
                    "Adding file {} to manifest consisting of {} blocks",
                    path,
                    blocks.len()
                )
            }
        }

        let mut working_directory = &mut self.root;

        if let Some(parent) = path.parent() {
            for component in parent.components() {
                let component_name = component.as_str();
                working_directory = match working_directory
                    .get_mut(component_name)
                    .map(|entry| &mut entry.specifics)
                {
                    Some(ManifestEntrySpecifics::Directory(directory_contents)) => {
                        directory_contents
                    }
                    _ => return Err(ManifestError::Create(path.to_owned())),
                };
            }
        }

        let file_name = path.file_name().unwrap().to_owned();
        working_directory.insert(file_name, entry);

        Ok(())
    }

    pub fn get(&self, path: impl AsRef<Utf8Path>) -> Result<&ManifestEntry, ManifestError> {
        let path = path.as_ref();
        let mut working_directory = &self.root;

        if let Some(parent) = path.parent() {
            for component in parent.components() {
                let component_name = component.as_str();
                working_directory = match working_directory
                    .get(component_name)
                    .map(|entry| &entry.specifics)
                {
                    Some(ManifestEntrySpecifics::Directory(directory_contents)) => {
                        directory_contents
                    }
                    _ => return Err(ManifestError::Create(path.to_owned())),
                };
            }
        }

        let filename = path
            .file_name()
            .ok_or_else(|| ManifestError::Create(path.to_owned()))?;
        working_directory
            .get(filename)
            .ok_or_else(|| ManifestError::Create(path.to_owned()))
    }

    pub fn list(
        &self,
        path: impl AsRef<Utf8Path>,
    ) -> Result<&HashMap<String, ManifestEntry>, ManifestError> {
        let path = path.as_ref();
        let mut working_directory = &self.root;

        if let Some(parent) = path.parent() {
            for component in parent.components() {
                let component_name = component.as_str();
                working_directory = match working_directory
                    .get(component_name)
                    .map(|entry| &entry.specifics)
                {
                    Some(ManifestEntrySpecifics::Directory(directory_contents)) => {
                        directory_contents
                    }
                    _ => return Err(ManifestError::Create(path.to_owned())),
                };
            }
        }

        let filename = path
            .file_name()
            .ok_or_else(|| ManifestError::Create(path.to_owned()))?;
        match working_directory
            .get(filename)
            .map(|entry| &entry.specifics)
        {
            Some(ManifestEntrySpecifics::Directory(directory_contents)) => Ok(directory_contents),
            _ => Err(ManifestError::Create(path.to_owned())),
        }
    }

    fn walk<'a>(
        &'a self,
        base_dir: impl AsRef<Utf8Path> + 'a,
    ) -> impl Iterator<Item = (Utf8PathBuf, &'a ManifestEntry)> {
        let base_dir = base_dir.as_ref();
        let mut queue = VecDeque::new();

        let entry = self.get(base_dir).unwrap();
        queue.push_back((base_dir.to_owned(), entry));

        std::iter::from_fn(move || {
            if let Some((current_path, current_entry)) = queue.pop_front() {
                if let ManifestEntrySpecifics::Directory(contents) = &current_entry.specifics {
                    for (name, child_entry) in contents {
                        let child_path = current_path.join(name);
                        queue.push_back((child_path, child_entry));
                    }
                }

                Some((current_path, current_entry))
            } else {
                None
            }
        })
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ManifestEntry {
    pub creation_date: Option<DateTime<Utc>>,
    pub modified_date: Option<DateTime<Utc>>,
    pub specifics: ManifestEntrySpecifics,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ManifestEntrySpecifics {
    Directory(HashMap<String, ManifestEntry>),
    File {
        hash: [u8; 32],
        blocks: Vec<BlockId>,
    },
}
