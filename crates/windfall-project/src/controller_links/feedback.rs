//! Declarative outbound surface protocol. Encoding happens off the audio thread.
use super::SlotTarget;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum FeedbackSource {
    Slot { slot: SlotTarget },
    Playing,
    Stopped,
    NoteRecordArmed,
    AutomationRecordArmed,
    MixerName { index: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum FeedbackMessage {
    Controller {
        channel: u8,
        number: u8,
    },
    NoteLed {
        channel: u8,
        key: u8,
    },
    PitchBend {
        channel: u8,
    },
    Display {
        prefix: Vec<u8>,
        suffix: Vec<u8>,
        width: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProfileFeedback {
    pub source: FeedbackSource,
    pub message: FeedbackMessage,
}

impl ProfileFeedback {
    pub fn problem(&self) -> Option<String> {
        let invalid = match &self.message {
            FeedbackMessage::Controller { channel, number } => {
                !(1..=16).contains(channel) || *number > 127
            }
            FeedbackMessage::NoteLed { channel, key } => !(1..=16).contains(channel) || *key > 127,
            FeedbackMessage::PitchBend { channel } => !(1..=16).contains(channel),
            FeedbackMessage::Display {
                prefix,
                suffix,
                width,
            } => {
                *width == 0
                    || *width > 64
                    || prefix.len() + suffix.len() > 64
                    || prefix.first() != Some(&0xf0)
                    || suffix.last() != Some(&0xf7)
                    || prefix
                        .iter()
                        .skip(1)
                        .chain(suffix.iter().take(suffix.len().saturating_sub(1)))
                        .any(|byte| *byte > 127)
            }
        };
        if invalid {
            return Some("Invalid surface feedback message.".into());
        }
        match self.source {
            FeedbackSource::Slot { slot } if slot.index() > super::MAX_SLOT_INDEX => {
                Some("Invalid feedback slot.".into())
            }
            FeedbackSource::MixerName { index } if index > super::MAX_SLOT_INDEX => {
                Some("Invalid feedback strip.".into())
            }
            _ => None,
        }
    }

    pub fn encode(&self, value: f32, text: &str) -> Vec<u8> {
        if self.problem().is_some() || !value.is_finite() {
            return Vec::new();
        }
        let value = value.clamp(0.0, 1.0);
        let seven = (value * 127.0).round() as u8;
        match &self.message {
            FeedbackMessage::Controller { channel, number } => {
                vec![0xb0 | (channel - 1), *number, seven]
            }
            FeedbackMessage::NoteLed { channel, key } => vec![
                0x90 | (channel - 1),
                *key,
                if value >= 0.5 { 127 } else { 0 },
            ],
            FeedbackMessage::PitchBend { channel } => {
                let value = (value * 16383.0).round() as u16;
                vec![
                    0xe0 | (channel - 1),
                    (value & 127) as u8,
                    (value >> 7) as u8,
                ]
            }
            FeedbackMessage::Display {
                prefix,
                suffix,
                width,
            } => {
                let mut bytes = prefix.clone();
                let mut text = text.chars();
                for _ in 0..*width {
                    bytes.push(text.next().map_or(b' ', |character| {
                        if character.is_ascii() && !character.is_control() {
                            character as u8
                        } else {
                            b'?'
                        }
                    }));
                }
                bytes.extend(suffix);
                bytes
            }
        }
    }
}
