use crate::{
    proto::Message,
    transport::packet::{MessageId, MAX_PACKET_PAYLOAD_SIZE},
};
use ringbuffer::{AllocRingBuffer, RingBuffer};
use std::num::NonZero;

pub const MARGIN_OF_OUT_OF_ORDER_ALLOWED: usize = 16;
pub const MAX_BODIES_PER_MESSAGE: usize = 512;

#[derive(Clone, Debug)]
pub struct PendingMessage {
    head_arrived: bool,
    compression: bool,
    max_index_seen: u16,
    total_bodies: Option<NonZero<u16>>,
    final_partial_body_length: Option<NonZero<u16>>,
    committed_bodies: [bool; MAX_BODIES_PER_MESSAGE],
    bodies: Vec<u8>,
}

impl Default for PendingMessage {
    fn default() -> Self {
        Self {
            bodies: Vec::new(),
            max_index_seen: 0,
            committed_bodies: [false; MAX_BODIES_PER_MESSAGE],
            total_bodies: None,
            final_partial_body_length: None,
            compression: false,
            head_arrived: false,
        }
    }
}

#[derive(Debug)]
pub struct MessageAssembler {
    /// Wrapping id for the last message pulled out
    current_message_id: MessageId,
    /// Storage for messages that have not been fully assembled yet
    pending_messages: AllocRingBuffer<Option<PendingMessage>>,
}

impl Default for MessageAssembler {
    fn default() -> Self {
        Self {
            current_message_id: 0,
            pending_messages: {
                let mut ring_buffer = AllocRingBuffer::new(MARGIN_OF_OUT_OF_ORDER_ALLOWED);
                ring_buffer.fill_default();
                ring_buffer
            },
        }
    }
}

impl MessageAssembler {
    pub fn head(&mut self, message_id: MessageId, body_count: NonZero<u16>, compression: bool) {
        let preassembled_message_entry_index = message_id.wrapping_sub(self.current_message_id);

        if let Some(entry) = self
            .pending_messages
            .get_mut(preassembled_message_entry_index as usize)
        {
            let entry = entry.get_or_insert_with(PendingMessage::default);

            let calculated_bodies_size = body_count.get() as usize * MAX_PACKET_PAYLOAD_SIZE
                - entry
                    .final_partial_body_length
                    .map(|length| MAX_PACKET_PAYLOAD_SIZE - length.get() as usize)
                    .unwrap_or(0);

            if entry.bodies.len() > calculated_bodies_size {
                self.pending_messages[preassembled_message_entry_index as usize].take();
                return;
            }

            entry.bodies.resize(calculated_bodies_size, 0);
            entry.total_bodies = Some(body_count);
            entry.compression = compression;
            entry.head_arrived = true;
        }
    }

    pub fn body(&mut self, message_id: MessageId, index: u16, data: Vec<u8>) {
        let preassembled_message_entry_index = message_id.wrapping_sub(self.current_message_id);

        if let Some(entry) = self
            .pending_messages
            .get_mut(preassembled_message_entry_index as usize)
        {
            let entry = entry.get_or_insert_with(PendingMessage::default);

            if data.is_empty() {
                self.pending_messages[preassembled_message_entry_index as usize].take();
                return;
            }

            // No overflowing
            if data.len() > MAX_PACKET_PAYLOAD_SIZE {
                self.pending_messages[preassembled_message_entry_index as usize].take();
                return;
            }

            // If its less than [MAX_PACKET_PAYLOAD_SIZE] its a partial body and the final body
            if data.len() < MAX_PACKET_PAYLOAD_SIZE {
                if let Some(final_partial_body_length) = entry.final_partial_body_length {
                    // Final data length was already set but now its different, discard
                    if final_partial_body_length.get() as usize != data.len() {
                        self.pending_messages[preassembled_message_entry_index as usize].take();
                        return;
                    }
                }

                entry.final_partial_body_length = Some(NonZero::new(data.len() as u16).unwrap());
                entry
                    .total_bodies
                    .get_or_insert(NonZero::new(index + 1).unwrap());
            }

            if index as usize == MAX_BODIES_PER_MESSAGE - 1 {
                entry
                    .total_bodies
                    .get_or_insert(NonZero::new(index + 1).unwrap());
            }

            if let Some(total_bodies) = entry.total_bodies {
                if total_bodies.get() <= index {
                    self.pending_messages[preassembled_message_entry_index as usize].take();
                    return;
                }
            }

            entry.max_index_seen = index.max(entry.max_index_seen);

            // This is a complicated piece of code
            // If total bodies are known use that, otherwise use the highest known index
            // If we don't know what the final body is or the final segment is not partial, assume that in to our calculations
            let calculated_bodies_size = entry
                .total_bodies
                .map(|b| b.get() as usize)
                .unwrap_or(entry.max_index_seen as usize + 1)
                * MAX_PACKET_PAYLOAD_SIZE
                - entry
                    .final_partial_body_length
                    .map(|length| MAX_PACKET_PAYLOAD_SIZE - length.get() as usize)
                    .unwrap_or(0);

            entry.bodies.resize(calculated_bodies_size, 0);

            let chunk = entry
                .bodies
                .chunks_mut(MAX_PACKET_PAYLOAD_SIZE)
                .nth(index as usize)
                .unwrap();

            // This would happen in the event that the remote is lying in the Head
            if chunk.len() != data.len() {
                self.pending_messages[preassembled_message_entry_index as usize].take();
                return;
            }

            chunk.copy_from_slice(&data);
            entry.committed_bodies[index as usize] = true;
        }
    }

    pub fn is_finished(&self, message_id: MessageId) -> bool {
        let preassembled_message_entry_index = message_id.wrapping_sub(self.current_message_id);

        if let Some(entry) = self
            .pending_messages
            .get(preassembled_message_entry_index as usize)
            .into_iter()
            .flatten()
            .next()
        {
            // The head has to arrive AND all relevant bodies have to be committed
            return entry.head_arrived
                && entry
                    .committed_bodies
                    .iter()
                    .take(entry.total_bodies.unwrap().get() as usize)
                    .all(|b| *b);
        }

        false
    }

    fn next(&mut self) -> Option<(Vec<u8>, MessageId)> {
        if self.is_finished(self.current_message_id) {
            let entry = self.pending_messages.dequeue().flatten().unwrap();
            self.pending_messages.push(None);

            let bodies = if entry.compression {
                lz4_flex::decompress_size_prepended(&entry.bodies).ok()?
            } else {
                entry.bodies
            };

            self.current_message_id = self.current_message_id.wrapping_add(1);

            return Some((bodies, self.current_message_id.wrapping_sub(1)));
        }

        None
    }

    pub fn next_message(&mut self) -> Option<(MessageId, Message)> {
        let (bodies, message_id) = self.next()?;
        let (message, _) =
            bincode::serde::decode_from_slice(&bodies, bincode::config::standard()).ok()?;

        Some((message_id, message))
    }
}

// Code for the payload tracker is not pretty so we form a bunch of tests
#[cfg(test)]
mod tests {
    use super::MARGIN_OF_OUT_OF_ORDER_ALLOWED;
    use crate::{
        channel::assembler::MessageAssembler,
        transport::packet::{MessageId, MAX_PACKET_PAYLOAD_SIZE},
    };
    use std::num::NonZero;

    #[test]
    fn final_partial_segment() {
        let mut tracker = MessageAssembler::default();
        let bodies = vec![0, 1, 2, 3];

        tracker.head(0, NonZero::new(1).unwrap(), false);
        tracker.body(0, 0, bodies.clone());

        assert_eq!(Some((bodies, 0)), tracker.next());
    }

    #[test]
    fn final_full_body() {
        let mut tracker = MessageAssembler::default();
        let bodies = [vec![0; MAX_PACKET_PAYLOAD_SIZE]];

        tracker.head(0, NonZero::new(bodies.len() as u16).unwrap(), false);
        tracker.body(0, 0, bodies[0].clone());

        assert_eq!(Some((bodies.concat(), 0)), tracker.next());
    }

    #[test]
    fn full_and_partial_body() {
        let mut tracker = MessageAssembler::default();
        let bodies = [vec![0; MAX_PACKET_PAYLOAD_SIZE], vec![0, 1, 2, 3]];

        tracker.head(0, NonZero::new(bodies.len() as u16).unwrap(), false);
        tracker.body(0, 0, bodies[0].clone());
        tracker.body(0, 1, bodies[1].clone());

        assert_eq!(Some((bodies.concat(), 0)), tracker.next());
    }

    #[test]
    fn multiple_full_bodies() {
        let mut tracker = MessageAssembler::default();
        let bodies = [
            vec![0; MAX_PACKET_PAYLOAD_SIZE],
            vec![1; MAX_PACKET_PAYLOAD_SIZE],
        ];

        tracker.head(0, NonZero::new(bodies.len() as u16).unwrap(), false);
        tracker.body(0, 0, bodies[0].clone());
        tracker.body(0, 1, bodies[1].clone());

        assert_eq!(Some((bodies.concat(), 0)), tracker.next());
    }

    #[test]
    fn out_of_order_body() {
        let mut tracker = MessageAssembler::default();
        let bodies = [
            vec![0; MAX_PACKET_PAYLOAD_SIZE],
            vec![1; MAX_PACKET_PAYLOAD_SIZE],
            vec![2; MAX_PACKET_PAYLOAD_SIZE],
            vec![0, 1, 2, 3],
        ];

        tracker.head(0, NonZero::new(bodies.len() as u16).unwrap(), false);
        tracker.body(0, 1, bodies[1].clone());
        tracker.body(0, 0, bodies[0].clone());
        tracker.body(0, 3, bodies[3].clone());
        tracker.body(0, 2, bodies[2].clone());

        assert_eq!(Some((bodies.concat(), 0)), tracker.next());
    }

    #[test]
    fn out_of_order_head() {
        let mut tracker = MessageAssembler::default();
        let bodies = [
            vec![0; MAX_PACKET_PAYLOAD_SIZE],
            vec![1; MAX_PACKET_PAYLOAD_SIZE],
            vec![2; MAX_PACKET_PAYLOAD_SIZE],
            vec![0, 1, 2, 3],
        ];

        tracker.body(0, 0, bodies[0].clone());
        tracker.body(0, 1, bodies[1].clone());
        tracker.body(0, 2, bodies[2].clone());
        tracker.head(0, NonZero::new(bodies.len() as u16).unwrap(), false);
        tracker.body(0, 3, bodies[3].clone());

        assert_eq!(Some((bodies.concat(), 0)), tracker.next());
    }

    #[test]
    fn out_of_order_bodies_and_head() {
        let mut tracker = MessageAssembler::default();
        let bodies = [
            vec![0; MAX_PACKET_PAYLOAD_SIZE],
            vec![1; MAX_PACKET_PAYLOAD_SIZE],
            vec![2; MAX_PACKET_PAYLOAD_SIZE],
            vec![0, 1, 2, 3],
        ];

        tracker.body(0, 1, bodies[1].clone());
        tracker.head(0, NonZero::new(bodies.len() as u16).unwrap(), false);
        tracker.body(0, 0, bodies[0].clone());
        tracker.body(0, 3, bodies[3].clone());
        tracker.body(0, 2, bodies[2].clone());

        assert_eq!(Some((bodies.concat(), 0)), tracker.next());
    }

    #[test]
    fn multiple_messages_single_bodies() {
        let mut tracker = MessageAssembler::default();
        let bodies = [
            [vec![0; MAX_PACKET_PAYLOAD_SIZE]],
            [vec![1; MAX_PACKET_PAYLOAD_SIZE]],
        ];

        tracker.head(0, NonZero::new(bodies[0].len() as u16).unwrap(), false);
        tracker.body(0, 0, bodies[0][0].clone());

        tracker.head(1, NonZero::new(bodies[1].len() as u16).unwrap(), false);
        tracker.body(1, 0, bodies[1][0].clone());

        assert_eq!(Some((bodies[0].concat(), 0)), tracker.next());
        assert_eq!(Some((bodies[1].concat(), 1)), tracker.next());
    }

    #[test]
    fn multiple_messages_multiple_bodies() {
        let mut tracker = MessageAssembler::default();
        let bodies = [
            [
                vec![0; MAX_PACKET_PAYLOAD_SIZE],
                vec![1; MAX_PACKET_PAYLOAD_SIZE],
            ],
            [
                vec![2; MAX_PACKET_PAYLOAD_SIZE],
                vec![3; MAX_PACKET_PAYLOAD_SIZE],
            ],
        ];

        tracker.head(0, NonZero::new(bodies[0].len() as u16).unwrap(), false);
        tracker.body(0, 0, bodies[0][0].clone());
        tracker.body(0, 1, bodies[0][1].clone());

        tracker.head(1, NonZero::new(bodies[1].len() as u16).unwrap(), false);
        tracker.body(1, 0, bodies[1][0].clone());
        tracker.body(1, 1, bodies[1][1].clone());

        assert_eq!(Some((bodies[0].concat(), 0)), tracker.next());
        assert_eq!(Some((bodies[1].concat(), 1)), tracker.next());
    }

    #[test]
    fn too_many_pending_messages() {
        let mut tracker = MessageAssembler::default();
        let body = vec![0; MAX_PACKET_PAYLOAD_SIZE];

        for i in 0..MARGIN_OF_OUT_OF_ORDER_ALLOWED as MessageId {
            tracker.head(i, NonZero::new(1).unwrap(), false);
            tracker.body(i, 0, body.clone());
        }

        tracker.head(
            MARGIN_OF_OUT_OF_ORDER_ALLOWED as MessageId,
            NonZero::new(1).unwrap(),
            false,
        );
        tracker.body(MARGIN_OF_OUT_OF_ORDER_ALLOWED as MessageId, 0, body.clone());

        for i in 0..MARGIN_OF_OUT_OF_ORDER_ALLOWED as MessageId {
            assert_eq!(Some((body.clone(), i)), tracker.next());
        }

        assert_eq!(None, tracker.next());
    }

    #[test]
    fn incorrect_first_message_correct_second_message() {
        let mut tracker = MessageAssembler::default();
        let body = [vec![1, 2, 3, 4], vec![5, 6, 7, 8], vec![9, 10, 11, 12]];

        // The third call will invalidate and discard the first message
        tracker.head(0, NonZero::new(1).unwrap(), false);
        tracker.body(0, 0, body[0].clone());
        tracker.body(0, 1, body[1].clone());

        // Message works as expected
        tracker.head(1, NonZero::new(1).unwrap(), false);
        tracker.body(1, 0, body[2].clone());

        // We should be getting neither of them
        assert_eq!(None, tracker.next());
        assert_eq!(None, tracker.next());

        // Now we write a actual sensical message
        tracker.head(0, NonZero::new(1).unwrap(), false);
        tracker.body(0, 0, body[0].clone());

        // We should be getting both of them
        assert_eq!(Some((body[0].clone(), 0)), tracker.next());
        assert_eq!(Some((body[2].clone(), 1)), tracker.next());
    }

    #[test]
    fn incorrect_head_body_count() {
        let mut tracker = MessageAssembler::default();
        let bodies = [
            vec![0; MAX_PACKET_PAYLOAD_SIZE],
            vec![1; MAX_PACKET_PAYLOAD_SIZE],
            vec![2; MAX_PACKET_PAYLOAD_SIZE],
            vec![0, 1, 2, 3],
        ];

        tracker.body(0, 1, bodies[1].clone());
        tracker.body(0, 0, bodies[0].clone());
        tracker.body(0, 3, bodies[3].clone());
        tracker.body(0, 2, bodies[2].clone());
        tracker.head(0, NonZero::new(1).unwrap(), false);

        assert_eq!(None, tracker.next());
    }
}
