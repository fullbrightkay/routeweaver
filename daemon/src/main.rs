use channel::{
    initiate::channel_initiator, reader::channel_read_message, writer::channel_write_message,
};
use clap::Parser;
use config::Config;
use discover::{
    announcer, discoverer,
    driver::{udp_multicast::UdpMulticastDiscovery, Discovery},
    local_address_refresher,
};
use ipc::ipc_server;
use noise::generate_keys;
use routeweaver_common::Protocol;
use state::ServerState;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::{signal::ctrl_c, sync::mpsc};
use transport::{
    accepter::accepter, driver::Transport, handshake_continue::handshake_continue,
    initiate::connection_initiator, router::packet_router,
};

mod channel;
mod config;
mod discover;
mod error;
mod ipc;
mod noise;
mod proto;
mod state;

// mod runtime;
mod transport;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[arg(short, long)]
    config_location: PathBuf,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let config =
        toml::from_str::<Config>(&std::fs::read_to_string(&cli.config_location).unwrap()).unwrap();

    let keys = config.keys.unwrap_or_else(generate_keys);
    let (request_route_packet_tx, request_route_packet_rx) = mpsc::channel(100);
    let (request_write_message_tx, request_write_message_rx) = mpsc::channel(100);
    let (request_initiate_channel_tx, request_initiate_channel_rx) = mpsc::channel(100);
    let (request_update_message_status_tx, request_update_message_status_rx) = mpsc::channel(100);
    let (request_decode_message_segment_tx, request_decode_message_segment_rx) = mpsc::channel(100);

    let server_state = Arc::new(ServerState::new(
        config.anonymous,
        keys,
        request_route_packet_tx,
        request_write_message_tx,
        request_initiate_channel_tx,
        request_decode_message_segment_tx,
        request_update_message_status_tx,
    ));

    tracing::info!("Starting RouteWeaver v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!("This nodes public key is {}", server_state.keys.public);

    #[cfg(transport_tcp)]
    setup_transport::<transport::driver::tcp::TcpTransport>(
        server_state.clone(),
        &config.transport_config,
    )
    .await;
    #[cfg(transport_ws)]
    setup_transport::<transport::driver::ws::WsTransport>(
        server_state.clone(),
        &config.transport_config,
    )
    .await;
    #[cfg(transport_wss)]
    setup_transport::<transport::driver::wss::WssTransport>(
        server_state.clone(),
        &config.transport_config,
    )
    .await;

    if let Some(config) = config.discovery_config.get(UdpMulticastDiscovery::ID) {
        let discovery = Arc::new(
            UdpMulticastDiscovery::from_config(config.clone())
                .await
                .unwrap(),
        );

        tokio::spawn(announcer(server_state.clone(), discovery.clone()));
        tokio::spawn(discoverer(server_state.clone(), discovery));
    }

    tokio::spawn(packet_router(server_state.clone(), request_route_packet_rx));
    tokio::spawn(channel_write_message(
        server_state.clone(),
        request_write_message_rx,
        request_update_message_status_rx,
    ));
    tokio::spawn(channel_read_message(
        server_state.clone(),
        request_decode_message_segment_rx,
    ));
    tokio::spawn(channel_initiator(
        server_state.clone(),
        request_initiate_channel_rx,
    ));
    tokio::spawn(handshake_continue(server_state.clone()));

    for initial_peer in config.initial_peers {
        if let Some(sender) = server_state
            .request_initiate_connection
            .get(&initial_peer.protocol)
        {
            sender.send(initial_peer.address).await.unwrap();
        }
    }

    if !config.routing_only {
        ipc_server().await;
    } else {
        ctrl_c().await.unwrap();
    }

    tracing::info!("Shutting down");
}

async fn setup_transport<T: Transport>(
    server_state: Arc<ServerState>,
    config: &HashMap<Protocol, toml::Value>,
) {
    if let Some(config) = config.get(&T::PROTOCOL) {
        let transport = Arc::new(T::from_config(config.clone()).await.unwrap());
        let (request_initiate_connection_tx, request_initiate_connection_rx) = mpsc::channel(10);

        server_state
            .request_initiate_connection
            .insert_async(T::PROTOCOL, request_initiate_connection_tx)
            .await
            .expect("Transport should be unique");

        tokio::spawn(accepter(server_state.clone(), transport.clone()));

        tokio::spawn(local_address_refresher(
            server_state.clone(),
            transport.clone(),
        ));

        tokio::spawn(connection_initiator(
            server_state.clone(),
            transport.clone(),
            request_initiate_connection_rx,
        ));
    }
}
