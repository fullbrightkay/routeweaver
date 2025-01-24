use crate::{
    state::ServerState,
    transport::packet::{MessagePayload, MessageSegment},
};
use routeweaver_common::PublicKey;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

use super::{
    assembler::MessageAssembler, handle_message::handle_message, writer::RequestUpdateMessageStatus,
};

pub struct RequestDecodeMessageSegment {
    pub source: PublicKey,
    pub segment: MessageSegment,
}

pub async fn channel_read_message(
    server_state: Arc<ServerState>,
    mut request_decode_message_segment: mpsc::Receiver<RequestDecodeMessageSegment>,
) {
    let mut message_assemblers = HashMap::new();
    let mut buffer = vec![0; u16::MAX as usize];

    while let Some(RequestDecodeMessageSegment { source, segment }) =
        request_decode_message_segment.recv().await
    {
        let Some(mut transport_state) = server_state.transport_tracker.get_async(&source).await
        else {
            server_state.request_initiate_channel.send(source).await;

            continue;
        };
        let message_assembler: &mut MessageAssembler =
            message_assemblers.entry(source).or_default();

        match segment.payload {
            // Head segment containing concrete information on the message
            MessagePayload::Head {
                body_count,
                compression,
            } => {
                message_assembler.head(segment.id, body_count, compression);
            }
            // Body segment containing data
            MessagePayload::Body { index, data } => {
                message_assembler.body(segment.id, index, data);
            }
            // Confirms what actually made it so far
            MessagePayload::MessageProgress {
                confirmed_head,
                confirmed_bodies,
            } => {
                server_state
                    .request_update_message_status
                    .send(RequestUpdateMessageStatus {
                        message_id: segment.id,
                        destination: source,
                        head_status: confirmed_head,
                        body_status: confirmed_bodies,
                    })
                    .await
                    .unwrap();
            }
        }

        while let Some((_, message)) = message_assembler.next_message() {
            handle_message(server_state.clone(), source, message).await;
        }
    }
}
