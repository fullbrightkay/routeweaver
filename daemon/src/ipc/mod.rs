use routeweaver_common::ipc::{DAEMON_RPC_SOCKET, RPC_BASE_DIR};
use socket::{socket_handler, ApplicationTracker};
use std::{ops::Deref, sync::Arc};
use tokio::fs::{create_dir_all, remove_file};

mod socket;

pub async fn ipc_server() {
    let application_tracker = Arc::new(ApplicationTracker::default());
    create_dir_all(RPC_BASE_DIR.deref()).await.unwrap();
    let _ = remove_file(DAEMON_RPC_SOCKET.deref()).await;

    socket_handler(application_tracker).await;
}
