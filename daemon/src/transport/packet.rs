use arrayvec::ArrayVec;
use rangemap::RangeInclusiveSet;
use routeweaver_common::PublicKey;
use serde::{Deserialize, Serialize};
use std::num::NonZero;

pub const MAX_PACKET_PAYLOAD_SIZE: usize = 63 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Packet {
    pub source: PublicKey,
    /// This is [Option::None] when a [Peer] would like to hint its identity as a given [PublicKey].
    ///
    /// Note that packets with no destination must only live 1 hop, are optional to accept and send, and any kind of peer/node association should merely be a hint
    ///
    /// A peer is allowed to send this only once, and this will be dropped if the source for the packet already has any kind of tunnel building up
    pub destination: Option<PublicKey>,
    pub data: PacketData,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum PacketData {
    /// Sent for doing handshakes.
    ///
    /// The only response to a handshake is a handshake
    ///
    /// Contains only noise handshake data, results in [HANDSHAKE_MESSAGE]
    Handshake(ArrayVec<u8, 128>),
    /// Results in a [MessageSegment]. Done this way to deal with tamperings
    ///
    /// Contains only noise transport data
    MessageSegment(Vec<u8>),
}

pub type MessageId = u16;

#[derive(Serialize, Deserialize, Debug)]
pub struct MessageSegment {
    pub id: MessageId,
    pub payload: MessagePayload,
}

#[derive(Serialize, Deserialize, PartialEq, PartialOrd, Debug)]
pub enum MessagePayload {
    /// Info needed to properly decode messages
    Head {
        /// The number of segments in the message
        body_count: NonZero<u16>,
        /// Whether or not the message is compressed
        ///
        /// Message is entirely compressed as a unit but encrypted on each individual data segment
        compression: bool,
    },
    /// Actual message content
    Body {
        /// The index of the data being sent
        index: u16,
        /// The actual data being sent, parts of a [Message]
        data: Vec<u8>,
    },
    /// Sent by the recipient of the message, saying what has actually made it
    MessageProgress {
        /// Head has arrived
        confirmed_head: bool,
        /// Bodies that have arrived
        confirmed_bodies: RangeInclusiveSet<u16>,
    },
}
