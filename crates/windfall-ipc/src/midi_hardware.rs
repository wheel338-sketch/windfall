//! Native MIDI devices and runtime routing; independent of Standard MIDI Files.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_project::ChannelId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MidiHardwareSettings {
    /// Opaque backend port IDs. None means disabled; never select a fallback.
    pub input: Option<String>,
    pub output: Option<String>,
    /// None accepts all channels; otherwise 1..=16.
    pub input_channel: Option<u8>,
    /// Live note forwarding only, on 1..=16. No song/clock output.
    pub output_channel: u8,
}

impl Default for MidiHardwareSettings {
    fn default() -> Self {
        Self {
            input: None,
            output: None,
            input_channel: None,
            output_channel: 1,
        }
    }
}

impl MidiHardwareSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .input_channel
            .is_some_and(|channel| !(1..=16).contains(&channel))
            || !(1..=16).contains(&self.output_channel)
        {
            return Err("MIDI channels must be between 1 and 16.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MidiPort {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MidiHardwareState {
    pub settings: MidiHardwareSettings,
    pub inputs: Vec<MidiPort>,
    pub outputs: Vec<MidiPort>,
    pub input_connected: bool,
    pub output_connected: bool,
    /// Runtime-only explicit audition destination, cleared on project replacement/removal.
    pub target: Option<ChannelId>,
    /// Session document identity for guarded destination requests.
    #[ts(type = "number")]
    pub generation: u64,
    pub dropped_events: u32,
    pub error: Option<String>,
}

/// One note a MIDI input played into the audition destination, as the
/// engine accepted it. The UI keeps these in its played-note log, which
/// "Dump played notes" quantizes with the shared snap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MidiHardwareNote {
    /// The channel the note was played into.
    pub channel: ChannelId,
    pub key: u8,
    /// 1 to 127 for a key going down, 0 for a key coming up.
    pub velocity: u8,
}
