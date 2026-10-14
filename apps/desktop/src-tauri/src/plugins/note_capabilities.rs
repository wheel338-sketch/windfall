//! Prepared native capability publication. Audio updates fixed atomics at an
//! ownership boundary; settings reads them on the native control owner.
//!
//! A hosted plugin returns one summed output. Each of a note's pan and volume
//! therefore has one of two routes: the plugin applies it per note where it
//! offers that, and otherwise the channel applies it to the whole output
//! after the plugin. Either way the project pan law is applied exactly once,
//! so nothing here refuses a valid channel.
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
    /// The plugin failed while processing and is bypassed or silent. The
    /// owner reads this before it saves state without taking the plugin back.
    failed: AtomicBool,
}
impl Published {
    pub fn fail(&self, failed: bool) {
        self.failed.store(failed, Ordering::Release);
    }
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
    pub fn publish(&self, instances: bool, masks: [u32; 16]) {
        for (stored, mask) in self.masks.iter().zip(masks) {
            stored.store(mask, Ordering::Release);
        }
        self.instances.store(instances, Ordering::Release);
    }
    /// Who applies per-note pan and volume on this channel: the plugin
    /// where it has the per-note route, the channel after it otherwise.
    pub fn routes(&self, channel: u8) -> windfall_plugin_host::project_expression::Routes {
        let mask = self
            .masks
            .get(usize::from(channel))
            .map_or(0, |mask| mask.load(Ordering::Acquire));
        windfall_plugin_host::project_expression::Routes::of(
            self.instances.load(Ordering::Acquire),
            mask,
        )
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
            observed_channels: (0..16)
                .filter(|channel| self.channels.load(Ordering::Relaxed) & (1 << channel) != 0)
                .collect(),
            refusal: self
                .invalid_channel
                .load(Ordering::Acquire)
                .then_some(INVALID_CHANNEL),
        }
    }
}
const INVALID_CHANNEL: &str = "Invalid note MIDI channel: expected a zero-based channel from 0 through 15; the note was refused.";
impl NoteCapabilities {
    /// Who applies per-note pan and volume on this channel.
    pub fn routes(&self, channel: u8) -> windfall_plugin_host::project_expression::Routes {
        let channel = usize::from(channel.min(15));
        windfall_plugin_host::project_expression::Routes {
            pan: self.instances && self.native_pan[channel],
            volume: self.instances && self.native_volume[channel],
        }
    }
    /// The project pan law reaches every note on a valid channel: through
    /// the plugin's own per-note pan and volume where it has them, and on
    /// the channel after the plugin for whichever it lacks.
    pub fn require_pan_law(&self, channel: u8) -> Result<(), String> {
        if channel > 15 {
            return Err(INVALID_CHANNEL.into());
        }
        if !self.project_pan_law {
            return Err("Per-note project pan laws are unavailable in this hosting mode".into());
        }
        Ok(())
    }
    /// Pan and volume destinations always have a route, so only an invalid
    /// channel refuses them.
    pub fn require_expression(&self, channel: u8, _pan: bool, _volume: bool) -> Result<(), String> {
        if channel > 15 {
            return Err(INVALID_CHANNEL.into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use windfall_plugin_host::project_expression::Routes;
    #[test]
    fn every_valid_channel_has_one_route_for_pan_and_one_for_volume() {
        // The four native/channel combinations, with and without note
        // instances, on the channel the notes are actually on.
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
                    let want = Routes {
                        pan: instances && pan,
                        volume: instances && volume,
                    };
                    assert_eq!(published.routes(3), want);
                    // A channel the plugin offers nothing on is all channel-level.
                    assert_eq!(published.routes(0), Routes::CHANNEL);
                    assert_eq!(published.routes(16), Routes::CHANNEL);
                    let caps = published.snapshot();
                    assert_eq!(caps.routes(3), want);
                    // Whatever the routes, the law applies and both
                    // destinations can be mapped.
                    assert!(caps.require_pan_law(3).is_ok());
                    assert!(caps.require_pan_law(0).is_ok());
                    assert!(caps.require_expression(3, true, false).is_ok());
                    assert!(caps.require_expression(3, false, true).is_ok());
                    assert!(caps.require_expression(3, true, true).is_ok());
                    for refused in [
                        caps.require_pan_law(16),
                        caps.require_expression(16, true, true),
                    ] {
                        assert!(refused.unwrap_err().contains("Invalid note MIDI channel"));
                    }
                }
            }
        }
    }
}
