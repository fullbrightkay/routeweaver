use std::{path::PathBuf, sync::LazyLock};

use serde::{Deserialize, Serialize};

pub mod socket;
pub mod stream;

pub static RPC_BASE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    if cfg!(target_os = "linux") {
        PathBuf::from("/run/routeweaver")
    } else {
        unreachable!()
    }
});

pub static DAEMON_RPC_SOCKET: LazyLock<PathBuf> = LazyLock::new(|| RPC_BASE_DIR.join("ipc"));

pub static SERVICE_RPC_BASE_DIRECTORY: LazyLock<PathBuf> =
    LazyLock::new(|| RPC_BASE_DIR.join("service"));

pub static ACTIVE_STREAM_DIRECTORY: LazyLock<PathBuf> =
    LazyLock::new(|| SERVICE_RPC_BASE_DIRECTORY.join("streams"));

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct StreamAuthToken([u8; 32]);

impl StreamAuthToken {
    pub fn new(data: [u8; 32]) -> Self {
        Self(data)
    }
}
