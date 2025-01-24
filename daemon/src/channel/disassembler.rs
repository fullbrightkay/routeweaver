use crate::{
    proto::Message,
    transport::packet::{MessageId, MessagePayload, MAX_PACKET_PAYLOAD_SIZE},
};
use rangemap::RangeInclusiveSet;
use std::{collections::HashMap, num::NonZero, ops::RangeInclusive};

fn should_message_be_compressed(bytes: &[u8]) -> bool {
    if bytes.len() <= 20 {
        return false;
    }

    let mut freq_table = [0usize; u8::MAX as usize + 1];

    for byte in bytes {
        freq_table[*byte as usize] = freq_table[*byte as usize].saturating_add(1);
    }

    let bytes_length = bytes.len() as f32;

    let entropy = freq_table.into_iter().fold(0.0, |acc, count| {
        if count > 0 {
            let prob = count as f32 / bytes_length;
            acc - prob * prob.log2()
        } else {
            acc
        }
    });

    let max_entropy = bytes_length.log2();
    let normalized_entropy = entropy / max_entropy;

    normalized_entropy.clamp(0.0, 1.0) <= 0.5
}

struct PayloadTracker {
    payload: Vec<u8>,
    compression: bool,
    head_confirmed: bool,
    body_count: NonZero<u16>,
    bodies_confirmed: RangeInclusiveSet<u16>,
}

#[derive(Default)]
pub struct MessageDisassembler {
    packets: HashMap<MessageId, PayloadTracker>,
    next_message_id: MessageId,
    highest_message_id: MessageId,
}

impl MessageDisassembler {
    pub fn message(&mut self, message: Message) -> MessageId {
        let encoded_payload =
            bincode::serde::encode_to_vec(&message, bincode::config::standard()).unwrap();

        let (encoded_payload, compression) = if should_message_be_compressed(&encoded_payload) {
            (lz4_flex::compress_prepend_size(&encoded_payload), true)
        } else {
            (encoded_payload, false)
        };

        let body_count = encoded_payload.len().div_ceil(MAX_PACKET_PAYLOAD_SIZE);
        let message_id = self.highest_message_id.wrapping_add(1);

        self.packets.insert(
            message_id,
            PayloadTracker {
                payload: encoded_payload,
                compression,
                head_confirmed: false,
                body_count: NonZero::new(body_count.try_into().unwrap()).unwrap(),
                bodies_confirmed: RangeInclusiveSet::new(),
            },
        );

        message_id
    }

    pub fn head_status(&mut self, message_id: MessageId, head_status: bool) {
        if let Some(tracker) = self.packets.get_mut(&message_id) {
            tracker.head_confirmed = head_status;
        }
    }

    pub fn body_status(
        &mut self,
        message_id: MessageId,
        body_status: impl IntoIterator<Item = RangeInclusive<u16>>,
    ) {
        if let Some(tracker) = self.packets.get_mut(&message_id) {
            tracker.bodies_confirmed.clear();
            tracker.bodies_confirmed.extend(body_status);
        }
    }

    pub fn is_message_confirmed(&self, message_id: MessageId) -> bool {
        if let Some(tracker) = self.packets.get(&message_id) {
            return tracker.head_confirmed
                && (0..tracker.body_count.get())
                    .all(|index| tracker.bodies_confirmed.contains(&index));
        }

        false
    }

    pub fn payloads(&mut self) -> Option<(MessageId, Vec<MessagePayload>)> {
        if let Some(tracker) = self.packets.get(&self.next_message_id) {
            let mut payloads = Vec::new();

            if !tracker.head_confirmed {
                payloads.push(MessagePayload::Head {
                    body_count: tracker.body_count,
                    compression: tracker.compression,
                });
            }

            for index in 0..tracker.body_count.get() {
                if tracker.bodies_confirmed.contains(&index) {
                    continue;
                }

                payloads.push(MessagePayload::Body {
                    data: tracker
                        .payload
                        .iter()
                        .skip(index as usize * MAX_PACKET_PAYLOAD_SIZE)
                        .take(MAX_PACKET_PAYLOAD_SIZE)
                        .copied()
                        .collect(),
                    index,
                });
            }

            if payloads.is_empty() {
                self.packets.remove(&self.next_message_id);
                self.next_message_id = self.next_message_id.wrapping_add(1);

                return None;
            }

            return Some((self.next_message_id, payloads));
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::MessageDisassembler;
    use crate::{
        channel::disassembler::should_message_be_compressed, proto::Message,
        transport::packet::MessagePayload,
    };
    use std::num::NonZero;

    #[test]
    fn message_entropy() {
        let buffer = vec![0; 1000];

        assert!(should_message_be_compressed(&buffer));
    }

    #[test]
    fn nonconfirmed_message() {
        let mut tracker = MessageDisassembler::default();
        tracker.message(Message::RequestPeerSuggestion);

        assert_eq!(
            tracker.payloads(),
            Some((
                0,
                vec![
                    MessagePayload::Head {
                        body_count: NonZero::new(1).unwrap(),
                        compression: false
                    },
                    MessagePayload::Body {
                        data: bincode::serde::encode_to_vec(
                            &Message::RequestPeerSuggestion,
                            bincode::config::standard()
                        )
                        .unwrap(),
                        index: 0
                    },
                ]
            ))
        );
    }

    #[test]
    fn confirmed_head() {
        let mut tracker = MessageDisassembler::default();
        tracker.message(Message::RequestPeerSuggestion);
        tracker.head_status(0, true);

        assert_eq!(
            tracker.payloads(),
            Some((
                0,
                vec![MessagePayload::Body {
                    index: 0,
                    data: bincode::serde::encode_to_vec(
                        &Message::RequestPeerSuggestion,
                        bincode::config::standard()
                    )
                    .unwrap()
                }]
            ))
        );
    }

    #[test]
    fn confirmed_body() {
        let mut tracker = MessageDisassembler::default();
        tracker.message(Message::RequestPeerSuggestion);
        tracker.body_status(0, [0..=0]);

        assert_eq!(
            tracker.payloads(),
            Some((
                0,
                vec![MessagePayload::Head {
                    body_count: NonZero::new(1).unwrap(),
                    compression: false
                }]
            ))
        );
    }

    #[test]
    fn confirmed_head_and_body() {
        let mut tracker = MessageDisassembler::default();
        tracker.message(Message::RequestPeerSuggestion);
        tracker.head_status(0, true);
        tracker.body_status(0, [0..=0]);

        assert_eq!(tracker.payloads(), None);
    }

    #[test]
    fn multiple_nonconfirmed_messages() {
        let mut tracker = MessageDisassembler::default();
        tracker.message(Message::RequestPeerSuggestion);
        tracker.message(Message::RequestPeerSuggestion);

        assert_eq!(
            tracker.payloads(),
            Some((
                0,
                vec![
                    MessagePayload::Head {
                        body_count: NonZero::new(1).unwrap(),
                        compression: false
                    },
                    MessagePayload::Body {
                        index: 0,
                        data: bincode::serde::encode_to_vec(
                            &Message::RequestPeerSuggestion,
                            bincode::config::standard()
                        )
                        .unwrap()
                    },
                ]
            ))
        );

        tracker.head_status(0, true);
        tracker.body_status(0, [0..=0]);
        tracker.payloads();

        assert_eq!(
            tracker.payloads(),
            Some((
                1,
                vec![
                    MessagePayload::Head {
                        body_count: NonZero::new(1).unwrap(),
                        compression: false
                    },
                    MessagePayload::Body {
                        index: 0,
                        data: bincode::serde::encode_to_vec(
                            &Message::RequestPeerSuggestion,
                            bincode::config::standard()
                        )
                        .unwrap()
                    }
                ]
            ))
        );
    }
}
