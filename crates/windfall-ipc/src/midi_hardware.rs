//! Native MIDI devices and runtime routing; independent of Standard MIDI Files.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_project::{
    AutomationTarget, ChannelId, ControlSource, ControllerLink, ControllerProfile, LinkTarget,
};

/// Inputs that can be open besides the main one.
pub const MAX_EXTRA_MIDI_INPUTS: usize = 7;
/// The furthest recorded notes may be moved to make up for input latency.
pub const MAX_RECORD_OFFSET_MS: f32 = 500.0;
/// The coarsest grid recorded notes may be snapped to: four bars of 4/4.
pub const MAX_RECORD_QUANTIZE_TICKS: u32 = 960 * 16;

/// A further MIDI input, open at the same time as the main one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MidiInputSettings {
    /// Opaque backend port ID.
    pub id: String,
    /// The controller profile assigned to this input, by its id.
    #[serde(default)]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MidiHardwareSettings {
    /// Opaque backend port IDs. None means disabled; never select a fallback.
    pub input: Option<String>,
    pub output: Option<String>,
    /// None accepts all channels; otherwise 1..=16.
    pub input_channel: Option<u8>,
    /// The channel live notes are forwarded on, 1..=16.
    pub output_channel: u8,
    /// The controller profile assigned to the main input, by its id.
    pub input_profile: Option<String>,
    /// Further inputs, at most [`MAX_EXTRA_MIDI_INPUTS`], each open at once
    /// with the main one. Their notes and controllers are read alike.
    pub extra_inputs: Vec<MidiInputSettings>,
    /// Sends MIDI clock, start, continue and stop to the output while the
    /// transport plays.
    pub send_clock: bool,
    /// Follows start, stop and tempo of a clock an input sends.
    pub follow_clock: bool,
}

impl Default for MidiHardwareSettings {
    fn default() -> Self {
        Self {
            input: None,
            output: None,
            input_channel: None,
            output_channel: 1,
            input_profile: None,
            extra_inputs: Vec::new(),
            send_clock: false,
            follow_clock: false,
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
        if self.extra_inputs.len() > MAX_EXTRA_MIDI_INPUTS {
            return Err(format!(
                "At most {} MIDI inputs can be open at once.",
                MAX_EXTRA_MIDI_INPUTS + 1
            ));
        }
        for (at, extra) in self.extra_inputs.iter().enumerate() {
            let repeated = self.input.as_deref() == Some(extra.id.as_str())
                || self.extra_inputs[..at]
                    .iter()
                    .any(|other| other.id == extra.id);
            if extra.id.is_empty() || repeated {
                return Err("Each MIDI input can be enabled once.".into());
            }
        }
        let profiles = std::iter::once(&self.input_profile)
            .chain(self.extra_inputs.iter().map(|extra| &extra.profile));
        for profile in profiles.flatten() {
            if profile.is_empty() || profile.len() > 64 {
                return Err("A controller profile's id is 1 to 64 characters.".into());
            }
        }
        Ok(())
    }

    /// The ports to open for input, the main one first, each with the
    /// profile assigned to it.
    pub fn inputs(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.input
            .iter()
            .map(|id| (id.as_str(), self.input_profile.as_deref()))
            .chain(
                self.extra_inputs
                    .iter()
                    .map(|extra| (extra.id.as_str(), extra.profile.as_deref())),
            )
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
    /// The main input is open.
    pub input_connected: bool,
    pub output_connected: bool,
    /// Runtime-only explicit audition destination, cleared on project replacement/removal.
    pub target: Option<ChannelId>,
    /// Session document identity for guarded destination requests.
    #[ts(type = "number")]
    pub generation: u64,
    pub dropped_events: u32,
    pub error: Option<String>,
    /// The IDs of every input that is open, the main one included.
    #[serde(default)]
    pub connected_inputs: Vec<String>,
    /// MIDI clock is being sent right now.
    #[serde(default)]
    pub clock_running: bool,
    /// Messages of MIDI output channels that were lost before they could
    /// be sent.
    #[serde(default)]
    pub output_dropped: u32,
    /// The tempo of the clock an input is sending, while one is.
    #[serde(default)]
    pub external_bpm: Option<f32>,
}

/// What the app remembers about controllers for every project.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ControllerSettings {
    /// Holds a linked control still until the hardware reaches its value.
    /// A link can overrule this either way.
    pub pickup: bool,
    /// Links that hold in every project. They move positions and run
    /// commands; they cannot name one project's ids.
    pub links: Vec<ControllerLink>,
    /// Moves every recorded note by this many milliseconds, to make up for
    /// the delay of the input. Negative is earlier.
    pub record_offset_ms: f32,
    /// Snaps the start of each recorded note to a grid of this many ticks.
    /// 0 records notes where they were played.
    pub record_quantize_ticks: u32,
}

impl ControllerSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !self.record_offset_ms.is_finite() || self.record_offset_ms.abs() > MAX_RECORD_OFFSET_MS
        {
            return Err(format!(
                "The recording offset is within {MAX_RECORD_OFFSET_MS} ms either way."
            ));
        }
        if self.record_quantize_ticks > MAX_RECORD_QUANTIZE_TICKS {
            return Err("The recording grid is too coarse.".into());
        }
        match windfall_project::controller_links::links_problem(&self.links, None) {
            Some(problem) => Err(problem),
            None => Ok(()),
        }
    }
}

/// A pass of linking controls to hardware that is under way.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ControllerLearn {
    /// What is being linked, in the order the hardware will be asked for.
    pub targets: Vec<LinkTarget>,
    /// How many of them have their hardware control already.
    pub linked: u32,
    /// The links go to the app's settings instead of the project.
    pub global: bool,
    /// Controls moved on screen are added to `targets` as they are moved.
    pub multi: bool,
}

/// Recording played notes into a pattern.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteRecordState {
    pub armed: bool,
    /// Notes captured in the pass that is running, not yet in the pattern.
    pub pending: u32,
    /// Notes that could not be kept because the pass grew too long.
    pub dropped: u32,
}

/// Recording control movements as automation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AutomationRecordState {
    pub armed: bool,
    /// Controls that have moved in the pass that is running.
    pub targets: u32,
    /// Movements captured in the pass, not yet written as automation.
    pub pending: u32,
}

/// The rolling log of everything played.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScoreLogState {
    /// Finished notes in the log.
    pub notes: u32,
    /// How far back the log reaches, in seconds.
    pub seconds: f32,
}

/// Everything the controller UI shows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ControllerState {
    pub settings: ControllerSettings,
    /// Every profile that can be assigned to an input: the built-in ones,
    /// then the user's.
    pub profiles: Vec<ControllerProfile>,
    pub learn: Option<ControllerLearn>,
    /// The hardware control that last sent anything.
    pub last_input: Option<ControlSource>,
    /// The control last moved on screen, which "link last tweaked" links.
    pub last_tweaked: Option<AutomationTarget>,
    /// Targets a link is holding still until its hardware reaches them.
    pub waiting: Vec<AutomationTarget>,
    /// How far positions have been moved by the bank commands.
    pub bank: u32,
    pub note_record: NoteRecordState,
    pub automation_record: AutomationRecordState,
    pub score_log: ScoreLogState,
    /// Controller messages lost to a full queue.
    pub dropped: u32,
    pub error: Option<String>,
    /// Session document identity, for guarded requests.
    #[ts(type = "number")]
    pub generation: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_inputs() -> MidiHardwareSettings {
        MidiHardwareSettings {
            input: Some("keys".into()),
            extra_inputs: vec![MidiInputSettings {
                id: "knobs".into(),
                profile: None,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn settings_refuse_repeated_inputs_and_too_many() {
        let mut settings = two_inputs();
        assert!(settings.validate().is_ok());
        let inputs: Vec<(&str, Option<&str>)> = settings.inputs().collect();
        assert_eq!(inputs, [("keys", None), ("knobs", None)]);
        settings.extra_inputs[0].id = "keys".into();
        assert!(settings.validate().is_err());
        settings.extra_inputs[0].id = String::new();
        assert!(settings.validate().is_err());
        settings.extra_inputs = (0..=MAX_EXTRA_MIDI_INPUTS)
            .map(|index| MidiInputSettings {
                id: format!("extra {index}"),
                profile: None,
            })
            .collect();
        assert!(settings.validate().is_err());
        settings.extra_inputs.pop();
        assert!(settings.validate().is_ok());
        settings.input_profile = Some(String::new());
        assert!(settings.validate().is_err());
        settings.input_profile = Some("korg-nanokontrol2".into());
        assert!(settings.validate().is_ok());
        settings.output_channel = 17;
        assert!(settings.validate().is_err());
    }

    #[test]
    fn a_settings_file_from_before_the_new_fields_still_loads() {
        let old: MidiHardwareSettings = serde_json::from_str(
            r#"{"input":"a","output":null,"inputChannel":3,"outputChannel":2}"#,
        )
        .unwrap();
        assert_eq!(old.input.as_deref(), Some("a"));
        assert_eq!((old.input_channel, old.output_channel), (Some(3), 2));
        assert!(old.extra_inputs.is_empty() && !old.send_clock && !old.follow_clock);
        assert_eq!(old.input_profile, None);
        let back: MidiHardwareSettings =
            serde_json::from_str(&serde_json::to_string(&two_inputs()).unwrap()).unwrap();
        assert_eq!(back, two_inputs());
        let controllers: ControllerSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(controllers, ControllerSettings::default());
    }

    #[test]
    fn controller_settings_are_bounded_and_global_links_cannot_name_ids() {
        use windfall_project::{ControlMap, MidiControl, SlotTarget};
        let source = ControlSource::Midi {
            device: None,
            channel: Some(1),
            control: MidiControl::Cc { number: 7 },
        };
        let mut settings = ControllerSettings {
            pickup: true,
            links: vec![ControllerLink {
                source: source.clone(),
                target: LinkTarget::Slot {
                    slot: SlotTarget::MasterVolume,
                },
                map: ControlMap::default(),
            }],
            record_offset_ms: -20.0,
            record_quantize_ticks: 240,
        };
        assert!(settings.validate().is_ok());
        settings.record_offset_ms = MAX_RECORD_OFFSET_MS + 1.0;
        assert!(settings.validate().is_err());
        settings.record_offset_ms = f32::NAN;
        assert!(settings.validate().is_err());
        settings.record_offset_ms = 0.0;
        settings.record_quantize_ticks = MAX_RECORD_QUANTIZE_TICKS + 1;
        assert!(settings.validate().is_err());
        settings.record_quantize_ticks = 0;
        settings.links[0].target = LinkTarget::Param {
            target: AutomationTarget::Tempo,
        };
        assert!(settings.validate().unwrap_err().contains("every project"));
    }
}
