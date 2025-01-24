use crate::{
    channel::{
        assembler::MessageAssembler,
        disassembler::MessageDisassembler,
        reader::RequestDecodeMessageSegment,
        writer::{RequestUpdateMessageStatus, RequestWriteMessage},
    },
    config::Keys,
    discover::LocalAddressTracker,
    transport::{packet::Packet, router::RequestRoutePacket, setup_connection::PeerTracker},
};
use routeweaver_common::{Address, ConnectionId, Peer, Protocol, PublicKey};
use snow::{HandshakeState, TransportState};
use tokio::sync::{
    broadcast,
    mpsc::{self, Sender},
};
use zeroize::Zeroizing;

/// Collection of random shit the server has to carry around for almost every task
pub struct ServerState {
    /// Try to preserve the nodes identity as much as possible
    pub anonymous: bool,
    /// Server keys
    pub keys: Keys,
    /// Tracks the servers current local addresses
    pub local_address_tracker: LocalAddressTracker,
    /// Tracks the states of active handshakes
    pub handshake_tracker: scc::HashMap<PublicKey, HandshakeState>,
    /// Tracks the states of active channels
    pub transport_tracker: scc::HashMap<PublicKey, TransportState>,
    /// Tracks currently connected peers
    pub peer_tracker: PeerTracker,
    // Requests that a connection be made to a given peer
    pub request_initiate_connection: scc::HashMap<Protocol, mpsc::Sender<Address>>,
    pub request_initiate_channel: mpsc::Sender<PublicKey>,
    /// Requests that a peer be given a packet
    pub request_write_packet: scc::HashMap<Peer, mpsc::Sender<Packet>>,
    /// Requests a message is sent, which involves encoding, encryption, and packeting
    pub request_write_message: mpsc::Sender<RequestWriteMessage>,
    /// Requests a calculation where a packet should go
    pub request_route_packet: mpsc::Sender<RequestRoutePacket>,

    pub request_decode_message_segment: mpsc::Sender<RequestDecodeMessageSegment>,
    pub request_update_message_status: mpsc::Sender<RequestUpdateMessageStatus>,
    /// Requests that the ipc server for applications processes some data
    pub request_receive_connection_data: scc::HashMap<ConnectionId, Sender<Zeroizing<Vec<u8>>>>,
    /// Notifies listeners that a new peer has connected, useful for reconsidering routing tables
    pub notification_new_peer_connection: broadcast::Sender<Peer>,
    /// Notifies a node has been successfully handshaked
    pub notification_handshaked_node: broadcast::Sender<PublicKey>,
}

impl ServerState {
    pub fn new(
        anonymous: bool,
        keys: Keys,
        request_route_packet: mpsc::Sender<RequestRoutePacket>,
        request_write_message: mpsc::Sender<RequestWriteMessage>,
        request_initiate_channel: mpsc::Sender<PublicKey>,
        request_decode_message_segment: mpsc::Sender<RequestDecodeMessageSegment>,
        request_update_message_status: mpsc::Sender<RequestUpdateMessageStatus>,
    ) -> Self {
        Self {
            anonymous,
            keys,
            local_address_tracker: LocalAddressTracker::default(),
            handshake_tracker: scc::HashMap::default(),
            transport_tracker: scc::HashMap::default(),
            peer_tracker: PeerTracker::default(),
            request_initiate_connection: scc::HashMap::default(),
            request_write_packet: scc::HashMap::default(),
            request_route_packet,
            request_initiate_channel,
            request_write_message,
            request_decode_message_segment,
            request_update_message_status,
            request_receive_connection_data: scc::HashMap::default(),
            notification_new_peer_connection: broadcast::channel(100).0,
            notification_handshaked_node: broadcast::channel(100).0,
        }
    }
}
