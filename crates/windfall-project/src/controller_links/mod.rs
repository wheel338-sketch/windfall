//! Controller links: what binds a control source to something it moves.
//!
//! A [`ControllerLink`] names a [`ControlSource`], a [`LinkTarget`] and the
//! [`ControlMap`] between them. Links saved with a project live in
//! [`ProjectSettings::controller_links`](crate::ProjectSettings) and are
//! edited by [`Command::SetControllerLinks`](crate::Command), so they undo,
//! redo and save like any other edit. Links that hold for every project are
//! kept by the app's own settings and use the same types.
//!
//! # The contract for sources and targets
//!
//! A source produces one number from 0 to 1 at a time. The map shapes that
//! number into the target's own 0 to 1, which means exactly what an
//! automation point's value means for the same
//! [`AutomationTarget`](crate::AutomationTarget): its
//! [`AutomationRange`](crate::AutomationRange) turns it into the target's
//! unit. Internal signals currently require project parameter targets and
//! absolute maps without pickup; Tempo is refused pending a clock feedback
//! contract. Hardware sources retain their slot/action and relative modes.
//!
//! - [`ControlSource::Midi`] is fed by hardware, away from the audio thread.
//! - [`ControlSource::Internal`] is evaluated by the engine at its owning
//!   mixer slot or Dashboard channel. Source ports, cycles and configuration
//!   are checked by `internal_controllers`. Source and target deletion both
//!   retire links in the same undo step. The hardware runtime never feeds it.
//!
//! A link to a [`LinkTarget::Param`] is removed in the same edit that
//! removes what it points at, exactly as automations are, and comes back
//! with it on undo.

mod feedback;
mod formula;
mod profile;
mod runtime;

pub use feedback::{FeedbackMessage, FeedbackSource, ProfileFeedback};
pub use formula::{Formula, MAX_FORMULA_BYTES, MAX_FORMULA_OPS};
pub use profile::{
    ControllerProfile, MAX_PROFILE_CONTROLS, ProfileControl, builtin_profiles, profile_for_port,
};
pub use runtime::{CompiledMap, LinkState, Motion, PICKUP_WINDOW, SourceInput};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::command::{ChannelPatch, Command, EffectSlotPatch, MixerTrackPatch, SettingsPatch};
use crate::model::{AutomationTarget, ChannelId, Project, TrackId};
use crate::plugin::PluginTarget;

/// The most links one project, or the app's own settings, may hold.
pub const MAX_CONTROLLER_LINKS: usize = 256;
/// The longest smoothing time of a link, in milliseconds.
pub const MAX_SMOOTHING_MS: f32 = 5_000.0;
/// The longest device name a link stores, in bytes.
pub const MAX_DEVICE_NAME_BYTES: usize = 256;
/// The highest position a [`SlotTarget`] may name.
pub const MAX_SLOT_INDEX: u32 = 127;
/// How many positions one bank step moves a [`SlotTarget`].
pub const SLOT_BANK_SIZE: u32 = 8;

/// One binding of a source to a target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ControllerLink {
    pub source: ControlSource,
    pub target: LinkTarget,
    #[serde(default)]
    pub map: ControlMap,
}

/// Where a link's value comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum ControlSource {
    /// A message from a MIDI input.
    Midi {
        /// The display name of the input port. `None` takes the message
        /// from any enabled input. Two ports with one name count as one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        device: Option<String>,
        /// The MIDI channel, 1 to 16. `None` takes any channel.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        channel: Option<u8>,
        control: MidiControl,
    },
    /// A control signal produced inside the project: output `output` of the
    /// effect or Dashboard channel with the id `controller`. The checked
    /// project validates its owner and output; the audio engine evaluates it.
    Internal { controller: u32, output: u8 },
}

/// Which message of a MIDI input a source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum MidiControl {
    /// A continuous controller, 0 to 127.
    Cc { number: u8 },
    /// A key or pad: 1 while it is down, 0 when it is let go.
    Note { key: u8 },
    /// The pitch wheel, with its full fourteen bits.
    PitchBend,
    /// Channel pressure (aftertouch).
    ChannelPressure,
}

/// What a link moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum LinkTarget {
    /// One thing of this project, by its id. Goes with it when it goes.
    Param { target: AutomationTarget },
    /// A command of the app, run when the source goes from off to on.
    Action { action: ControlAction },
    /// Something found by its position, so the link means something in
    /// every project: see [`SlotTarget`].
    Slot { slot: SlotTarget },
}

/// A command a button can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ControlAction {
    Play,
    Stop,
    TogglePlay,
    ToggleLoop,
    ToggleMetronome,
    ToggleNoteRecord,
    ToggleAutomationRecord,
    Undo,
    Redo,
    PatternPrevious,
    PatternNext,
    /// Moves every [`SlotTarget`] position down by [`SLOT_BANK_SIZE`].
    BankPrevious,
    /// Moves every [`SlotTarget`] position up by [`SLOT_BANK_SIZE`].
    BankNext,
    PanicMidi,
    /// Launch the assigned pad using the project quantization.
    PerformancePad {
        track: u8,
        scene: u8,
    },
    PerformanceStop {
        track: u8,
    },
    PerformanceStopAll,
}

/// A target named by where it is rather than by what it is. `index` counts
/// from 0 and the bank offset of the session is added to it. A position
/// with nothing at it moves nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum SlotTarget {
    /// The fader of the mixer track at this position, not counting the master.
    MixerVolume {
        index: u32,
    },
    MixerPan {
        index: u32,
    },
    /// The volume of the channel at this position of the channel rack.
    ChannelVolume {
        index: u32,
    },
    ChannelPan {
        index: u32,
    },
    MasterVolume,
    Tempo,
}

impl SlotTarget {
    /// The target this position holds in `project` with the bank moved by
    /// `bank` positions, or `None` when nothing is there.
    pub fn resolve(self, project: &Project, bank: u32) -> Option<AutomationTarget> {
        let at = |index: u32| index.checked_add(bank).map(|index| index as usize);
        let track = |index: u32| -> Option<TrackId> {
            let tracks = project.mixer.tracks.iter();
            let mut inserts = tracks.filter(|track| track.id != TrackId::MASTER);
            Some(inserts.nth(at(index)?)?.id)
        };
        let channel =
            |index: u32| -> Option<ChannelId> { Some(project.channels.get(at(index)?)?.id) };
        Some(match self {
            SlotTarget::MixerVolume { index } => AutomationTarget::TrackVolume {
                track: track(index)?,
            },
            SlotTarget::MixerPan { index } => AutomationTarget::TrackPan {
                track: track(index)?,
            },
            SlotTarget::ChannelVolume { index } => AutomationTarget::ChannelVolume {
                channel: channel(index)?,
            },
            SlotTarget::ChannelPan { index } => AutomationTarget::ChannelPan {
                channel: channel(index)?,
            },
            SlotTarget::MasterVolume => AutomationTarget::TrackVolume {
                track: TrackId::MASTER,
            },
            SlotTarget::Tempo => AutomationTarget::Tempo,
        })
    }

    fn index(self) -> u32 {
        match self {
            SlotTarget::MixerVolume { index }
            | SlotTarget::MixerPan { index }
            | SlotTarget::ChannelVolume { index }
            | SlotTarget::ChannelPan { index } => index,
            SlotTarget::MasterVolume | SlotTarget::Tempo => 0,
        }
    }
}

/// How a source value is read before it is mapped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SourceMode {
    /// The value is a position: a fader, a knob with end stops, a wheel.
    #[default]
    Absolute,
    /// An endless knob that sends steps as a seven-bit two's complement:
    /// 1 is one step up, 127 is one step down.
    RelativeTwosComplement,
    /// An endless knob that sends a sign bit: 1 to 63 are steps up and
    /// 65 to 127 are 1 to 63 steps down.
    RelativeSignedBit,
    /// An endless knob that sends steps around 64: 65 is one step up and
    /// 63 is one step down.
    RelativeBinaryOffset,
    /// A button that flips the target between the two ends of the map each
    /// time it is pressed.
    Toggle,
}

/// Whether a link waits for the hardware to reach the target's value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PickupMode {
    /// As the MIDI settings say.
    #[default]
    Inherit,
    On,
    Off,
}

/// How a source value becomes a target value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ControlMap {
    /// The target's value, 0 to 1, when the shaped source is at 0.
    pub min: f32,
    /// The target's value when the shaped source is at 1. Below `min`, the
    /// link runs backwards.
    pub max: f32,
    /// An expression of `x`, the source value, giving the shaped value:
    /// see [`Formula`]. `None` leaves the source as it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub formula: Option<String>,
    /// How long the target takes to cover about two thirds of the way to a
    /// new value, 0 to [`MAX_SMOOTHING_MS`]. 0 moves it at once.
    pub smoothing_ms: f32,
    pub pickup: PickupMode,
    pub mode: SourceMode,
}

impl Default for ControlMap {
    fn default() -> Self {
        Self {
            min: 0.0,
            max: 1.0,
            formula: None,
            smoothing_ms: 0.0,
            pickup: PickupMode::Inherit,
            mode: SourceMode::Absolute,
        }
    }
}

impl ControllerLink {
    /// What is wrong with the link on its own, whatever project it is in.
    pub fn problem(&self) -> Option<String> {
        match &self.source {
            ControlSource::Midi {
                device,
                channel,
                control,
            } => {
                if device
                    .as_ref()
                    .is_some_and(|name| name.is_empty() || name.len() > MAX_DEVICE_NAME_BYTES)
                {
                    return Some(
                        "a controller link names a device with no name or too long a name".into(),
                    );
                }
                if channel.is_some_and(|channel| !(1..=16).contains(&channel)) {
                    return Some("a controller link's MIDI channel is not 1 to 16".into());
                }
                let data = match *control {
                    MidiControl::Cc { number } => number,
                    MidiControl::Note { key } => key,
                    MidiControl::PitchBend | MidiControl::ChannelPressure => 0,
                };
                if data > 127 {
                    return Some("a controller link's controller or key is above 127".into());
                }
            }
            ControlSource::Internal { .. } => {}
        }
        if let LinkTarget::Slot { slot } = self.target
            && slot.index() > MAX_SLOT_INDEX
        {
            return Some("a controller link's position is too high".into());
        }
        if let LinkTarget::Action { action } = self.target {
            let invalid = match action {
                ControlAction::PerformancePad { track, scene } => {
                    usize::from(track) >= crate::stage_media::PERFORMANCE_TRACKS
                        || usize::from(scene) >= crate::stage_media::PERFORMANCE_SCENES
                }
                ControlAction::PerformanceStop { track } => {
                    usize::from(track) >= crate::stage_media::PERFORMANCE_TRACKS
                }
                _ => false,
            };
            if invalid {
                return Some("Performance controller action position is out of range".into());
            }
        }
        self.map.compile().err()
    }
}

/// What is wrong with a list of links for `project`, if anything. With no
/// project, only what does not depend on one is checked, which is the rule
/// for links kept outside a project: those may not use [`LinkTarget::Param`],
/// whose ids mean nothing elsewhere.
pub fn links_problem(links: &[ControllerLink], project: Option<&Project>) -> Option<String> {
    if links.len() > MAX_CONTROLLER_LINKS {
        return Some(format!(
            "there are more than {MAX_CONTROLLER_LINKS} controller links"
        ));
    }
    for (at, link) in links.iter().enumerate() {
        if let Some(problem) = link.problem() {
            return Some(problem);
        }
        if links[..at]
            .iter()
            .any(|other| other.source == link.source && other.target == link.target)
        {
            return Some("two controller links bind the same source to the same target".into());
        }
        if let LinkTarget::Param { target } = &link.target {
            match project {
                Some(project) if project.automation_range(target).is_some() => {}
                Some(_) => {
                    return Some(
                        "a controller link points at something that is not in the project".into(),
                    );
                }
                None => {
                    return Some(
                        "a link kept for every project cannot point at one project's control"
                            .into(),
                    );
                }
            }
        }
    }
    if let Some(project) = project {
        return crate::internal_controllers::links_problem(project, links);
    }
    if links
        .iter()
        .any(|link| matches!(link.source, ControlSource::Internal { .. }))
    {
        return Some("internal sources cannot be kept outside a project".into());
    }
    None
}

/// The command that gives `target` the value `value`, in the target's own
/// unit. `None` when the project has no such thing.
pub fn target_command(project: &Project, target: &AutomationTarget, value: f32) -> Option<Command> {
    project.automation_range(target)?;
    let plugin_param = |plugin: PluginTarget, index: u32| {
        let binding = project.plugin(plugin)?;
        let id = binding.parameters.get(index as usize)?.id;
        Some(Command::SetPluginParam {
            target: plugin,
            id,
            value,
        })
    };
    Some(match *target {
        AutomationTarget::ChannelVolume { channel } => Command::UpdateChannel {
            id: channel,
            patch: ChannelPatch {
                volume: Some(value),
                ..ChannelPatch::default()
            },
        },
        AutomationTarget::ChannelPan { channel } => Command::UpdateChannel {
            id: channel,
            patch: ChannelPatch {
                pan: Some(value),
                ..ChannelPatch::default()
            },
        },
        AutomationTarget::TrackVolume { track } => Command::UpdateMixerTrack {
            id: track,
            patch: MixerTrackPatch {
                volume: Some(value),
                ..MixerTrackPatch::default()
            },
        },
        AutomationTarget::TrackPan { track } => Command::UpdateMixerTrack {
            id: track,
            patch: MixerTrackPatch {
                pan: Some(value),
                ..MixerTrackPatch::default()
            },
        },
        AutomationTarget::TrackParam { track, param } => Command::SetTrackParam {
            id: track,
            param,
            value,
        },
        AutomationTarget::SendGain { track, target } => Command::SetSend {
            from: track,
            to: target,
            gain: Some(value),
        },
        AutomationTarget::SidechainGain { track, target } => Command::SetSidechain {
            from: track,
            to: target,
            gain: Some(value),
        },
        AutomationTarget::EffectParam {
            track,
            effect,
            param,
        } => {
            let plugin = PluginTarget::Effect { effect };
            if project.plugin(plugin).is_some() {
                return plugin_param(plugin, param);
            }
            Command::SetEffectParam {
                track,
                effect,
                param,
                value,
            }
        }
        AutomationTarget::EffectMix { track, effect } => Command::UpdateEffect {
            track,
            effect,
            patch: EffectSlotPatch {
                enabled: None,
                mix: Some(value),
            },
        },
        AutomationTarget::InstrumentParam { channel, param } => {
            let plugin = PluginTarget::Instrument { channel };
            if project.plugin(plugin).is_some() {
                return plugin_param(plugin, param);
            }
            Command::SetInstrumentParam {
                channel,
                param,
                value,
            }
        }
        AutomationTarget::Tempo => Command::UpdateSettings {
            patch: SettingsPatch {
                tempo_bpm: Some(f64::from(value)),
                ..SettingsPatch::default()
            },
        },
    })
}

/// The targets `command` sets a value of, read against the project the
/// command is about to run on: the reverse of [`target_command`]. This is
/// how "the control that was just moved" is known. Commands that do more
/// than set values, such as adding or removing things, give nothing.
pub fn command_targets(project: &Project, command: &Command) -> Vec<AutomationTarget> {
    let mut found = Vec::new();
    collect_targets(project, command, &mut found);
    found.retain(|target| project.automation_range(target).is_some());
    found
}

fn collect_targets(project: &Project, command: &Command, found: &mut Vec<AutomationTarget>) {
    let mut add = |target: AutomationTarget| {
        if !found.contains(&target) {
            found.push(target);
        }
    };
    match command {
        Command::UpdateChannel { id, patch } => {
            if patch.volume.is_some() {
                add(AutomationTarget::ChannelVolume { channel: *id });
            }
            if patch.pan.is_some() {
                add(AutomationTarget::ChannelPan { channel: *id });
            }
        }
        Command::UpdateMixerTrack { id, patch } => {
            if patch.volume.is_some() {
                add(AutomationTarget::TrackVolume { track: *id });
            }
            if patch.pan.is_some() {
                add(AutomationTarget::TrackPan { track: *id });
            }
        }
        Command::SetTrackParam { id, param, .. } => add(AutomationTarget::TrackParam {
            track: *id,
            param: *param,
        }),
        Command::SetSend {
            from,
            to,
            gain: Some(_),
        } => add(AutomationTarget::SendGain {
            track: *from,
            target: *to,
        }),
        Command::SetSidechain {
            from,
            to,
            gain: Some(_),
        } => add(AutomationTarget::SidechainGain {
            track: *from,
            target: *to,
        }),
        Command::SetEffectParam {
            track,
            effect,
            param,
            ..
        } => add(AutomationTarget::EffectParam {
            track: *track,
            effect: *effect,
            param: *param,
        }),
        Command::UpdateEffect {
            track,
            effect,
            patch,
        } if patch.mix.is_some() => add(AutomationTarget::EffectMix {
            track: *track,
            effect: *effect,
        }),
        Command::SetInstrumentParam { channel, param, .. } => {
            add(AutomationTarget::InstrumentParam {
                channel: *channel,
                param: *param,
            });
        }
        Command::SetPluginParam { target, id, .. } => {
            let Some(binding) = project.plugin(*target) else {
                return;
            };
            let Some(index) = binding.parameters.iter().position(|param| param.id == *id) else {
                return;
            };
            let param = index as u32;
            match *target {
                PluginTarget::Rack { .. } => {},
                PluginTarget::Instrument { channel } => {
                    add(AutomationTarget::InstrumentParam { channel, param });
                }
                PluginTarget::Effect { effect } => {
                    let mut tracks = project.mixer.tracks.iter();
                    let on = tracks.find(|track| track.effect(effect).is_some());
                    if let Some(track) = on {
                        add(AutomationTarget::EffectParam {
                            track: track.id,
                            effect,
                            param,
                        });
                    }
                }
            }
        }
        Command::UpdateSettings { patch } if patch.tempo_bpm.is_some() => {
            add(AutomationTarget::Tempo);
        }
        Command::Batch { commands, .. } => {
            for command in commands {
                collect_targets(project, command, found);
            }
        }
        _ => {}
    }
}

/// `links` without those that point at a target `gone` picks out.
pub(crate) fn without_targets(
    links: &[ControllerLink],
    gone: impl Fn(&AutomationTarget) -> bool,
) -> Option<Vec<ControllerLink>> {
    let stays = |link: &&ControllerLink| match &link.target {
        LinkTarget::Param { target } => !gone(target),
        LinkTarget::Action { .. } | LinkTarget::Slot { .. } => true,
    };
    let kept: Vec<ControllerLink> = links.iter().filter(stays).cloned().collect();
    (kept.len() != links.len()).then_some(kept)
}

/// The project's links after an edit, when the edit left any behind: a
/// link to an effect follows the effect to the track it now is on, and a
/// link whose target is gone goes too. `None` when every link still holds.
pub(crate) fn following(project: &Project) -> Option<Vec<ControllerLink>> {
    let links = &project.settings.controller_links;
    if links.is_empty() {
        return None;
    }
    let home = |effect| {
        let mut tracks = project.mixer.tracks.iter();
        tracks
            .find(|track| track.effect(effect).is_some())
            .map(|track| track.id)
    };
    let mut changed = false;
    let mut kept: Vec<ControllerLink> = Vec::with_capacity(links.len());
    for link in links {
        let mut link = link.clone();
        if let ControlSource::Internal { controller, output } = link.source
            && !crate::internal_controllers::source(project, controller)
                .is_some_and(|(_, owner)| usize::from(output) < owner.ports().len())
        {
            changed = true;
            continue;
        }
        if let LinkTarget::Param { target } = &mut link.target {
            let before = *target;
            match target {
                AutomationTarget::EffectParam { track, effect, .. }
                | AutomationTarget::EffectMix { track, effect } => {
                    if let Some(now) = home(*effect) {
                        *track = now;
                    }
                }
                _ => {}
            }
            changed |= before != *target;
            if project.automation_range(target).is_none() {
                changed = true;
                continue;
            }
        }
        // Following a move can make two links the same; one of them stays.
        if kept
            .iter()
            .any(|other| other.source == link.source && other.target == link.target)
        {
            changed = true;
            continue;
        }
        kept.push(link);
    }
    changed.then_some(kept)
}

#[cfg(test)]
mod tests;
