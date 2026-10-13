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
}
#[derive(Default)]
pub(super) struct Published {
    instances: AtomicBool,
    masks: [AtomicU32; 16],
}
impl Published {
    pub fn publish(&self, instances: bool, masks: [u32; 16]) {
        for (stored, mask) in self.masks.iter().zip(masks) { stored.store(mask, Ordering::Release); }
        self.instances.store(instances, Ordering::Release);
    }
    pub fn supports_pan_law(&self, channel: u8) -> bool {
        let mask=self.masks[usize::from(channel.min(15))].load(Ordering::Acquire);
        self.instances.load(Ordering::Acquire) && mask & NoteExpressionKind::Pan.mask()!=0 && mask & NoteExpressionKind::Volume.mask()!=0
    }
    pub fn snapshot(&self) -> NoteCapabilities {
        let masks = self.masks.each_ref().map(|mask| mask.load(Ordering::Acquire));
        NoteCapabilities {
            instances: self.instances.load(Ordering::Acquire),
            project_pan_law: true,
            native_pan: masks.map(|mask| mask & NoteExpressionKind::Pan.mask() != 0),
            native_volume: masks.map(|mask| mask & NoteExpressionKind::Volume.mask() != 0),
        }
    }
}
impl NoteCapabilities {
    pub fn require_pan_law(&self, channel: u8) -> Result<(), String> {
        let channel = usize::from(channel.min(15));
        if !self.project_pan_law { return Err("Per-note project pan laws are unavailable in this hosting mode".into()); }
        if self.instances && self.native_pan[channel] && self.native_volume[channel] { Ok(()) }
        else { Err("This hosted instrument lacks native Pan, Volume or note-instance support on the selected MIDI channel; per-note project pan law is unavailable.".into()) }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_pan_law_requires_all_three_capabilities_on_the_actual_channel() {
        for instances in [false,true] {
            for pan in [false,true] { for volume in [false,true] {
                let published = Published::default();
                let mut masks=[0;16];
                masks[3]=(if pan {NoteExpressionKind::Pan.mask()} else {0}) | (if volume {NoteExpressionKind::Volume.mask()} else {0});
                published.publish(instances,masks);
                let caps=published.snapshot();
                assert_eq!(caps.require_pan_law(3).is_ok(),instances && pan && volume);
                assert!(caps.require_pan_law(0).is_err());
            } }
        }
    }
}
