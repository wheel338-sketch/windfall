//! Prepared native capability publication. Audio updates fixed atomics at an
//! ownership boundary; settings reads them on the native control owner.
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windfall_plugin_host::NoteExpressionKind;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteCapabilities {
    pub instances: bool,
    pub project_pan_law: bool,
    pub native_pan: [bool; 16],
    pub native_volume: [bool; 16],
    pub observed_channels: Vec<u8>,
    pub refusal: Option<&'static str>,
}
#[derive(Default)]
pub(super) struct Published {
    instances: AtomicBool,
    masks: [AtomicU32; 16],
    channels: AtomicU32,
    invalid_channel: AtomicBool,
}
impl Published {
    pub fn publish(&self, instances: bool, masks: [u32; 16]) {
        for (stored, mask) in self.masks.iter().zip(masks) {
            stored.store(mask, Ordering::Release);
        }
        self.instances.store(instances, Ordering::Release);
    }
    pub fn supports_pan_law(&self, channel: u8) -> bool {
        let Some(mask) = self.masks.get(usize::from(channel)) else {
            return false;
        };
        let mask = mask.load(Ordering::Acquire);
        self.instances.load(Ordering::Acquire)
            && mask & NoteExpressionKind::Pan.mask() != 0
            && mask & NoteExpressionKind::Volume.mask() != 0
    }
    pub fn observe_channel(&self, channel: u8) {
        if channel < 16 {
            self.channels.fetch_or(1 << channel, Ordering::Relaxed);
        } else {
            self.refuse_channel();
        }
    }
    pub fn refuse_channel(&self) {
        self.invalid_channel.store(true, Ordering::Release);
    }
    pub fn snapshot(&self) -> NoteCapabilities {
        let masks = self
            .masks
            .each_ref()
            .map(|mask| mask.load(Ordering::Acquire));
        NoteCapabilities {
            instances: self.instances.load(Ordering::Acquire),
            project_pan_law: true,
            native_pan: masks.map(|mask| mask & NoteExpressionKind::Pan.mask() != 0),
            native_volume: masks.map(|mask| mask & NoteExpressionKind::Volume.mask() != 0),
            observed_channels: (0..16).filter(|channel| self.channels.load(Ordering::Relaxed) & (1 << channel) != 0).collect(),
            refusal: self.invalid_channel.load(Ordering::Acquire).then_some("Invalid note MIDI channel: expected a zero-based channel from 0 through 15; the note was refused."),
        }
    }
}
impl NoteCapabilities {
    pub fn require_pan_law(&self, channel: u8) -> Result<(), String> {
        if channel > 15 {
            return Err("Invalid note MIDI channel: expected a zero-based channel from 0 through 15; the note was refused.".into());
        }
        let channel = usize::from(channel);
        if !self.project_pan_law {
            return Err("Per-note project pan laws are unavailable in this hosting mode".into());
        }
        if self.instances && self.native_pan[channel] && self.native_volume[channel] {
            Ok(())
        } else {
            Err("This hosted instrument lacks native Pan, Volume or note-instance support on the selected MIDI channel; per-note project pan law is unavailable.".into())
        }
    }
    pub fn require_expression(&self, channel: u8, pan: bool, volume: bool) -> Result<(), String> {
        if channel > 15 {
            return self.require_pan_law(channel);
        }
        let channel = usize::from(channel);
        if self.instances
            && (!pan || self.native_pan[channel])
            && (!volume || self.native_volume[channel])
        {
            Ok(())
        } else {
            Err(format!(
                "The requested note expression is unavailable on MIDI channel {} (Pan: {}, Volume: {}, note instances: {}).",
                channel + 1,
                self.native_pan[channel],
                self.native_volume[channel],
                self.instances
            ))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_pan_law_requires_all_three_capabilities_on_the_actual_channel() {
        for instances in [false, true] {
            for pan in [false, true] {
                for volume in [false, true] {
                    let published = Published::default();
                    let mut masks = [0; 16];
                    masks[3] = (if pan {
                        NoteExpressionKind::Pan.mask()
                    } else {
                        0
                    }) | (if volume {
                        NoteExpressionKind::Volume.mask()
                    } else {
                        0
                    });
                    published.publish(instances, masks);
                    let caps = published.snapshot();
                    assert_eq!(caps.require_pan_law(3).is_ok(), instances && pan && volume);
                    assert!(caps.require_pan_law(0).is_err());
                    assert_eq!(
                        caps.require_expression(3, true, false).is_ok(),
                        instances && pan
                    );
                    assert_eq!(
                        caps.require_expression(3, false, true).is_ok(),
                        instances && volume
                    );
                    assert!(!published.supports_pan_law(16));
                    assert!(
                        caps.require_pan_law(16)
                            .unwrap_err()
                            .contains("Invalid note MIDI channel")
                    );
                }
            }
        }
    }
}
