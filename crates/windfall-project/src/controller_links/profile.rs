//! Controller profiles: ready-made link sets for particular hardware.
//!
//! A profile is plain data. The ones built in below describe what each
//! device sends as shipped; a profile a user writes is the same structure
//! as JSON. Assigning a profile to an input turns its controls into links
//! from that input, with positions ([`SlotTarget`](super::SlotTarget)) and
//! commands ([`ControlAction`](super::ControlAction)) for targets, since a
//! profile has to mean something in every project.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{
    ControlAction, ControlMap, ControlSource, ControllerLink, FeedbackMessage, FeedbackSource,
    LinkTarget, MidiControl, ProfileFeedback, SlotTarget, SourceMode,
};

/// The most controls one profile describes.
pub const MAX_PROFILE_CONTROLS: usize = 256;

/// One control of a device and what it does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProfileControl {
    /// What the control is called on the device: "Fader 1", "Play".
    pub name: String,
    /// The MIDI channel it sends on, 1 to 16. `None` takes any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub channel: Option<u8>,
    pub control: MidiControl,
    /// A position or a command. A profile cannot name one project's ids.
    pub target: LinkTarget,
    #[serde(default)]
    pub map: ControlMap,
}

/// Ready-made support for one kind of controller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ControllerProfile {
    /// Stable identifier, lowercase letters, digits and dashes.
    pub id: String,
    pub name: String,
    /// Parts of port names this profile suits, compared without case.
    #[serde(default)]
    pub ports: Vec<String>,
    /// What the user has to set on the device for the profile to fit.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(as = "Option<String>", optional)]
    pub notes: String,
    pub controls: Vec<ProfileControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional)]
    pub feedback: Vec<ProfileFeedback>,
}

impl ControllerProfile {
    /// What is wrong with the profile, if anything.
    pub fn problem(&self) -> Option<String> {
        let id_ok = !self.id.is_empty()
            && self.id.len() <= 64
            && self
                .id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if !id_ok {
            return Some(
                "a controller profile's id is lowercase letters, digits and dashes".into(),
            );
        }
        if self.name.trim().is_empty() || self.name.len() > 128 {
            return Some("a controller profile needs a name of at most 128 bytes".into());
        }
        if self.controls.len() > MAX_PROFILE_CONTROLS {
            return Some(format!(
                "a controller profile has at most {MAX_PROFILE_CONTROLS} controls"
            ));
        }
        if self.ports.len() > 16
            || self
                .ports
                .iter()
                .any(|port| port.is_empty() || port.len() > 128)
        {
            return Some("a controller profile's port names are invalid".into());
        }
        if self.feedback.len() > MAX_PROFILE_CONTROLS {
            return Some("Too many surface feedback rules.".into());
        }
        if let Some(problem) = self.feedback.iter().find_map(ProfileFeedback::problem) {
            return Some(problem);
        }
        super::links_problem(&self.links(None), None)
    }

    /// The profile as links from the input named `device`, or from any
    /// input with `None`.
    pub fn links(&self, device: Option<&str>) -> Vec<ControllerLink> {
        self.controls
            .iter()
            .map(|control| ControllerLink {
                source: ControlSource::Midi {
                    device: device.map(str::to_owned),
                    channel: control.channel,
                    control: control.control,
                },
                target: control.target,
                map: control.map.clone(),
            })
            .collect()
    }

    /// True when the profile lists a part of this port's name.
    pub fn suits(&self, port: &str) -> bool {
        let port = port.to_lowercase();
        self.ports
            .iter()
            .any(|part| port.contains(&part.to_lowercase()))
    }
}

/// The built-in profile that suits a port with this name, if any.
pub fn profile_for_port(port: &str) -> Option<ControllerProfile> {
    builtin_profiles()
        .into_iter()
        .find(|profile| profile.suits(port))
}

fn control(
    name: impl Into<String>,
    channel: Option<u8>,
    control: MidiControl,
    target: LinkTarget,
    mode: SourceMode,
) -> ProfileControl {
    ProfileControl {
        name: name.into(),
        channel,
        control,
        target,
        map: ControlMap {
            mode,
            ..ControlMap::default()
        },
    }
}

fn action(
    name: &str,
    channel: Option<u8>,
    source: MidiControl,
    action: ControlAction,
) -> ProfileControl {
    control(
        name,
        channel,
        source,
        LinkTarget::Action { action },
        SourceMode::Absolute,
    )
}

fn slot(
    name: String,
    channel: Option<u8>,
    source: MidiControl,
    slot: SlotTarget,
    mode: SourceMode,
) -> ProfileControl {
    control(name, channel, source, LinkTarget::Slot { slot }, mode)
}

/// Every profile shipped with Windfall. None has been checked against the
/// physical device; each follows the message layout the device documents.
pub fn builtin_profiles() -> Vec<ControllerProfile> {
    vec![mackie_control(), nano_kontrol2(), general_midi_mixer()]
}

/// The Mackie Control protocol: touch faders send the pitch wheel on
/// channels 1 to 9, the eight encoders send relative steps with a sign bit
/// on controllers 16 to 23, and buttons send notes on channel 1.
fn mackie_control() -> ControllerProfile {
    let mut controls = Vec::new();
    for strip in 0..8_u8 {
        controls.push(slot(
            format!("Fader {}", strip + 1),
            Some(strip + 1),
            MidiControl::PitchBend,
            SlotTarget::MixerVolume {
                index: u32::from(strip),
            },
            SourceMode::Absolute,
        ));
        controls.push(slot(
            format!("V-Pot {}", strip + 1),
            Some(1),
            MidiControl::Cc { number: 16 + strip },
            SlotTarget::MixerPan {
                index: u32::from(strip),
            },
            SourceMode::RelativeSignedBit,
        ));
    }
    controls.push(slot(
        "Master fader".to_owned(),
        Some(9),
        MidiControl::PitchBend,
        SlotTarget::MasterVolume,
        SourceMode::Absolute,
    ));
    for (name, key, command) in [
        ("Bank left", 46, ControlAction::BankPrevious),
        ("Bank right", 47, ControlAction::BankNext),
        ("Undo", 70, ControlAction::Undo),
        ("Cycle", 86, ControlAction::ToggleLoop),
        ("Click", 89, ControlAction::ToggleMetronome),
        ("Stop", 93, ControlAction::Stop),
        ("Play", 94, ControlAction::Play),
        ("Record", 95, ControlAction::ToggleNoteRecord),
    ] {
        controls.push(action(name, Some(1), MidiControl::Note { key }, command));
    }
    let mut feedback = Vec::new();
    for strip in 0..8_u8 {
        feedback.push(ProfileFeedback {
            source: FeedbackSource::Slot {
                slot: SlotTarget::MixerVolume {
                    index: u32::from(strip),
                },
            },
            message: FeedbackMessage::PitchBend { channel: strip + 1 },
        });
        feedback.push(ProfileFeedback {
            source: FeedbackSource::MixerName {
                index: u32::from(strip),
            },
            message: FeedbackMessage::Display {
                prefix: vec![0xf0, 0, 0, 0x66, 0x14, 0x12, strip * 7],
                suffix: vec![0xf7],
                width: 7,
            },
        });
    }
    for (source, key) in [
        (FeedbackSource::Playing, 94),
        (FeedbackSource::Stopped, 93),
        (FeedbackSource::NoteRecordArmed, 95),
    ] {
        feedback.push(ProfileFeedback {
            source,
            message: FeedbackMessage::NoteLed { channel: 1, key },
        });
    }
    ControllerProfile {
        feedback,
        id: "mackie-control".to_owned(),
        name: "Mackie Control compatible".to_owned(),
        ports: vec!["mackie control".to_owned(), "mcu pro".to_owned()],
        notes: "Set the surface to Mackie Control mode and choose its MIDI output for fader, LED and display feedback."
            .to_owned(),
        controls,
    }
}

/// The Korg nanoKONTROL2 in its factory CC mode on MIDI channel 1.
fn nano_kontrol2() -> ControllerProfile {
    let mut controls = Vec::new();
    for strip in 0..8_u8 {
        controls.push(slot(
            format!("Fader {}", strip + 1),
            Some(1),
            MidiControl::Cc { number: strip },
            SlotTarget::MixerVolume {
                index: u32::from(strip),
            },
            SourceMode::Absolute,
        ));
        controls.push(slot(
            format!("Knob {}", strip + 1),
            Some(1),
            MidiControl::Cc { number: 16 + strip },
            SlotTarget::MixerPan {
                index: u32::from(strip),
            },
            SourceMode::Absolute,
        ));
    }
    for (name, number, command) in [
        ("Play", 41, ControlAction::Play),
        ("Stop", 42, ControlAction::Stop),
        ("Record", 45, ControlAction::ToggleNoteRecord),
        ("Cycle", 46, ControlAction::ToggleLoop),
        ("Track left", 58, ControlAction::BankPrevious),
        ("Track right", 59, ControlAction::BankNext),
        ("Marker left", 61, ControlAction::PatternPrevious),
        ("Marker right", 62, ControlAction::PatternNext),
    ] {
        controls.push(action(name, Some(1), MidiControl::Cc { number }, command));
    }
    let feedback = [
        (FeedbackSource::Playing, 41),
        (FeedbackSource::Stopped, 42),
        (FeedbackSource::NoteRecordArmed, 45),
    ]
    .into_iter()
    .map(|(source, number)| ProfileFeedback {
        source,
        message: FeedbackMessage::Controller { channel: 1, number },
    })
    .collect();
    ControllerProfile {
        feedback,
        id: "korg-nanokontrol2".to_owned(),
        name: "Korg nanoKONTROL2".to_owned(),
        ports: vec!["nanokontrol2".to_owned()],
        notes: "Use the factory CC mode on MIDI channel 1, with momentary buttons.".to_owned(),
        controls,
    }
}

/// Any surface that sends the standard volume (7) and pan (10) controllers,
/// one strip per MIDI channel.
fn general_midi_mixer() -> ControllerProfile {
    let mut controls = Vec::new();
    for strip in 0..16_u8 {
        controls.push(slot(
            format!("Volume {}", strip + 1),
            Some(strip + 1),
            MidiControl::Cc { number: 7 },
            SlotTarget::MixerVolume {
                index: u32::from(strip),
            },
            SourceMode::Absolute,
        ));
        controls.push(slot(
            format!("Pan {}", strip + 1),
            Some(strip + 1),
            MidiControl::Cc { number: 10 },
            SlotTarget::MixerPan {
                index: u32::from(strip),
            },
            SourceMode::Absolute,
        ));
    }
    ControllerProfile {
        feedback: Vec::new(),
        id: "general-midi-mixer".to_owned(),
        name: "Standard volume and pan controllers".to_owned(),
        ports: Vec::new(),
        notes: "One mixer track per MIDI channel: controller 7 is its fader and 10 its pan."
            .to_owned(),
        controls,
    }
}
