use crate::{
    proto::Message,
    state::ServerState,
    transport::{
        packet::{MessageId, MessageSegment, Packet, PacketData},
        router::RequestRoutePacket,
    },
};
use rangemap::{RangeInclusiveSet, RangeSet};
use routeweaver_common::PublicKey;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, oneshot, Notify},
    time::sleep,
};

use super::disassembler::MessageDisassembler;

pub struct RequestWriteMessageResponse {
    pub time_taken: Duration,
}

pub struct RequestWriteMessage {
    pub notify_sent: Option<oneshot::Sender<RequestWriteMessageResponse>>,
    pub destination: PublicKey,
    pub message: Message,
}

pub struct RequestUpdateMessageStatus {
    pub message_id: MessageId,
    pub destination: PublicKey,
    pub head_status: bool,
    pub body_status: RangeInclusiveSet<u16>,
}

pub async fn channel_write_message(
    server_state: Arc<ServerState>,
    mut request_write_message: mpsc::Receiver<RequestWriteMessage>,
    mut request_update_message_status: mpsc::Receiver<RequestUpdateMessageStatus>,
) {
    let mut message_disassemblers = HashMap::new();
    let mut notify_callbacks = HashMap::default();
    let mut encryption_buffer = vec![0; u16::MAX as usize];

    loop {
        tokio::select! {
            biased;
            // Queue up message
            v = request_write_message.recv() => {
                if let Some(RequestWriteMessage { notify_sent, destination, message }) = v {
                    handle_write_message(&mut message_disassemblers, &mut notify_callbacks,
                        notify_sent, destination, message);
                } else {
                    break;
                }
            },
            // Update the message status
            v = request_update_message_status.recv() => {
                if let Some(RequestUpdateMessageStatus { message_id, destination, head_status, body_status }) = v {
                    handle_update_message_status(&mut message_disassemblers, &mut notify_callbacks,
                        message_id, destination, head_status, body_status);
                } else {
                    break;
                }
            }
            // Occasional wakeup for writing
            _ = sleep(Duration::from_secs(1)) => {}
        }

        // Go through and send again all queued messages
        for (node, message_disassemblers) in message_disassemblers.iter_mut() {
            if let Some((message_id, payloads)) = message_disassemblers.payloads() {
                if let Some(mut transport_state) =
                    server_state.transport_tracker.get_async(node).await
                {
                    for payload in payloads {
                        let segment = MessageSegment {
                            id: message_id,
                            payload,
                        };

                        let amount = transport_state
                            .write_message(
                                &bincode::serde::encode_to_vec(
                                    segment,
                                    bincode::config::standard(),
                                )
                                .unwrap(),
                                &mut encryption_buffer,
                            )
                            .unwrap();

                        let packet = Packet {
                            source: server_state.keys.public,
                            destination: Some(*node),
                            data: PacketData::MessageSegment(encryption_buffer[..amount].to_vec()),
                        };

                        server_state
                            .request_route_packet
                            .send(RequestRoutePacket {
                                origin: None,
                                packet,
                            })
                            .await
                            .unwrap();
                    }
                } else {
                    tracing::debug!("Tried sending message segments to node {}, but a channel for them doesn't exist. Requesting creation", node);

                    server_state
                        .request_initiate_channel
                        .send(*node)
                        .await
                        .unwrap();
                }
            }
        }
    }
}

#[inline]
pub fn handle_write_message(
    message_disassemblers: &mut HashMap<PublicKey, MessageDisassembler>,
    notify_callbacks: &mut HashMap<MessageId, oneshot::Sender<RequestWriteMessageResponse>>,
    notify_sent: Option<oneshot::Sender<RequestWriteMessageResponse>>,
    destination: PublicKey,
    message: Message,
) {
    let message_disassembler = message_disassemblers.entry(destination).or_default();
    let message_id = message_disassembler.message(message);

    if let Some(notify_sent) = notify_sent {
        notify_callbacks.insert(message_id, notify_sent);
    }
}

#[inline]
pub fn handle_update_message_status(
    message_disassemblers: &mut HashMap<PublicKey, MessageDisassembler>,
    notify_callbacks: &mut HashMap<MessageId, oneshot::Sender<RequestWriteMessageResponse>>,
    message_id: MessageId,
    destination: PublicKey,
    head_status: bool,
    body_status: RangeInclusiveSet<u16>,
) {
    if let Some(message_disassembler) = message_disassemblers.get_mut(&destination) {
        message_disassembler.head_status(message_id, head_status);
        message_disassembler.body_status(message_id, body_status);

        if message_disassembler.is_message_confirmed(message_id) {
            if let Some(notify_callback) = notify_callbacks.remove(&message_id) {
                // Don't really care if anyone is actually listening on the other end
                let _ = notify_callback.send(RequestWriteMessageResponse {
                    time_taken: Duration::default(),
                });
            }
        }
    }
}
