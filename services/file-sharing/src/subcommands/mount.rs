use crate::manifest::Manifest;
use camino::{Utf8Path, Utf8PathBuf};
use futures_util::Stream;
use std::{ffi::OsStr, future::Future, path::Path};

struct Fuse {
    manifest: Manifest,
}

pub async fn mount(mount_path: impl AsRef<Path>) {}
