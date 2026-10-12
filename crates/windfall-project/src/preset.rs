//! Preset files: the settings of a channel, of one instrument or effect, or
//! of a mixer track, kept as a reusable file.
//!
//! A preset is Windfall's own format. Every preset file has the same
//! envelope: a schema name, a version, a kind, a display name and a manifest
//! of the audio files it needs. The kind says which one body it carries:
//!
//! - **Channel** ([`ChannelPreset`]): what a rack channel sounds like. Its
//!   level, pan, voice and timing settings, and its sound: sampler settings,
//!   a built-in instrument's settings, or a hosted instrument with its
//!   opaque native state.
//! - **Plugin** ([`PluginPreset`]): the state of one processor, built-in or
//!   hosted, instrument or effect.
//! - **Mixer track** ([`MixerTrackPreset`](crate::MixerTrackPreset)): the
//!   mixer's own preset, carried unchanged so that one file type covers all
//!   three. It is loaded with
//!   [`Command::ApplyMixerTrackPreset`](crate::Command::ApplyMixerTrackPreset).
//!
//! # What a preset leaves to its destination
//!
//! A preset describes a sound, not a place in a project. A channel keeps
//! its id, rack group, mute, solo and mixer track when a preset is loaded
//! onto it, and an effect keeps its slot, its mix and whether it is
//! bypassed. Nothing in a preset file is a project id: a hosted plugin's
//! binding is stored against a placeholder and pointed at the destination
//! when it loads, and a sampler's audio is named by its entry in the
//! manifest, never by a sample id.
//!
//! # Audio
//!
//! A sampler preset lists its audio in `assets`, each with a path relative
//! to the preset file. Whoever writes the file puts the audio there, and
//! whoever reads it resolves the path against the file's folder, adds the
//! audio to the destination project and passes the sample it became to
//! [`Command::ApplyChannelPreset`](crate::Command::ApplyChannelPreset).
//!
//! # Automation
//!
//! Automation that addresses a setting by its index means nothing once the
//! processor behind the index is another one. Loading a preset keeps such
//! automation only when the processor stays the same: the same built-in
//! kind, or the same hosted plugin with the same parameter at that index.
//! Everything else that moved the replaced settings is removed, with its
//! clips, in the same undo step. Automation of what the destination keeps,
//! such as a channel's volume or an effect's mix, is never touched.
//!
//! # Reading older files
//!
//! [`Preset::from_json`] reads a preset written by an older build even when
//! the settings of a built-in processor have changed since: a setting that
//! was renamed is carried to its new name ([`PARAM_RENAMES`]), one this
//! build does not have is dropped, one the file does not have takes its
//! default, and a value outside its range is brought into it. Each of these
//! is reported. A file of a newer preset version is refused by name.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::model::{
    Channel, ChannelId, ChannelSource, EffectId, EffectKind, EffectParams, InstrumentKind,
    InstrumentParams, MAX_GAIN, Project, SampleAsset, SampleId, SamplePath, TrackId,
    relative_path_problem,
};
use crate::plugin::{PluginBinding, PluginTarget};
use crate::{ChannelTiming, ChannelVoiceSettings, MixerTrackPreset};

/// What the `schema` field of a preset file says.
pub const PRESET_SCHEMA: &str = "windfall.preset";
/// The newest preset version this build reads and the one it writes.
pub const PRESET_VERSION: u32 = 1;
/// The longest preset or asset name, in bytes.
pub const MAX_PRESET_NAME_BYTES: usize = 256;
/// The most audio files one preset lists.
pub const MAX_PRESET_ASSETS: usize = 8;
/// The largest audio file a preset lists.
pub const MAX_PRESET_ASSET_BYTES: u64 = 1024 * 1024 * 1024;
/// The largest preset file a reader takes in.
pub const MAX_PRESET_FILE_BYTES: usize = 256 * 1024 * 1024;

/// Which body a preset carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PresetKind {
    Channel,
    Plugin,
    MixerTrack,
}

impl PresetKind {
    /// The name of the kind as a sentence uses it.
    pub fn label(self) -> &'static str {
        match self {
            PresetKind::Channel => "channel",
            PresetKind::Plugin => "plugin",
            PresetKind::MixerTrack => "mixer track",
        }
    }
}

/// One audio file a preset needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PresetAsset {
    /// What the body calls the asset. Unique within the preset.
    pub id: u32,
    /// The name the sample gets in a project.
    pub name: String,
    /// Where the audio is, relative to the preset file, with forward
    /// slashes. It stays inside the preset's folder.
    pub path: String,
    /// The size of the file, for telling a replaced file from the original.
    pub bytes: u64,
}

/// A preset file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Preset {
    /// Always [`PRESET_SCHEMA`].
    pub schema: String,
    pub version: u32,
    pub kind: PresetKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<PresetAsset>>", optional)]
    pub assets: Vec<PresetAsset>,
    /// The body of a [`PresetKind::Channel`] preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub channel: Option<ChannelPreset>,
    /// The body of a [`PresetKind::Plugin`] preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plugin: Option<PluginPreset>,
    /// The body of a [`PresetKind::MixerTrack`] preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub mixer_track: Option<MixerTrackPreset>,
}

/// What a rack channel sounds like.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ChannelPreset {
    /// The channel's name and color, which loading restores only when asked.
    pub name: String,
    pub color: u32,
    pub volume: f32,
    pub pan: f32,
    #[serde(
        default,
        deserialize_with = "crate::channel_voice::deserialize_settings"
    )]
    #[ts(as = "Option<ChannelVoiceSettings>", optional)]
    pub voice: ChannelVoiceSettings,
    #[serde(default)]
    #[ts(as = "Option<ChannelTiming>", optional)]
    pub timing: ChannelTiming,
    /// The sound. A sampler's `sample` is always empty here: its audio is
    /// the asset `sample_asset` names.
    pub source: ChannelSource,
    /// The id of the asset a sampler plays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sample_asset: Option<u32>,
    /// The hosted instrument that replaces the built-in one in `source`,
    /// with its opaque state. Its target is a placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plugin: Option<PluginBinding>,
    /// Built-in modular graph, including nested nodes and saved controls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub rack: Option<crate::RackGraph>,
}

/// The state of one processor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginPreset {
    pub state: PluginPresetState,
}

/// What a [`PluginPreset`] holds: the settings of a built-in processor, or
/// a hosted plugin's binding with its opaque native state.
// Built-in settings stay inline, as they are everywhere else in the model.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum PluginPresetState {
    Effect {
        params: EffectParams,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        impulse: Option<crate::ConvolverImpulse>,
    },
    Instrument {
        params: InstrumentParams,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        asset: Option<windfall_dsp::InstrumentAsset>,
    },
    /// The binding's target is a placeholder: loading points it at the
    /// destination.
    Hosted {
        instrument: bool,
        binding: PluginBinding,
    },
}

/// Which plugin a preset belongs to, for listing presets beside plugins.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PresetPluginIdentity {
    /// "builtin", "clap" or "vst3".
    pub format: String,
    /// A hosted plugin's identifier, or a built-in's kind as files spell it.
    pub id: String,
    pub instrument: bool,
}

/// Audio for a sampler preset that the project does not have yet: the name
/// and the file of the sample to add.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PresetAudio {
    pub name: String,
    pub path: SamplePath,
}

/// A setting of a built-in processor that has had another name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamRename {
    /// The processor's `type`, as files spell it.
    pub processor: &'static str,
    pub from: &'static str,
    pub to: &'static str,
}

/// Every rename a preset reader carries values across. A setting that is
/// renamed gets an entry here in the same change, so presets saved before
/// it keep the value.
pub const PARAM_RENAMES: &[ParamRename] = &[];

/// The processor behind a channel's instrument settings or an effect slot,
/// as far as automation by index is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Processor {
    Sampler,
    Instrument(InstrumentKind),
    Effect(EffectKind),
    /// The ids of the plugin's parameters, in table order, and whether each
    /// can be automated.
    Hosted {
        format: String,
        id: String,
        parameters: Vec<(u32, bool)>,
    },
}

impl Processor {
    fn hosted(binding: &PluginBinding) -> Self {
        Processor::Hosted {
            format: binding.format.clone(),
            id: binding.id.clone(),
            parameters: binding
                .parameters
                .iter()
                .map(|param| (param.id, param.automatable && !param.read_only))
                .collect(),
        }
    }

    pub(crate) fn of_source(source: &ChannelSource, binding: Option<&PluginBinding>) -> Self {
        match (source, binding) {
            (ChannelSource::Sampler(_), _) => Processor::Sampler,
            (ChannelSource::Instrument { .. }, Some(binding)) => Self::hosted(binding),
            (ChannelSource::Instrument { params, .. }, None) => {
                Processor::Instrument(params.kind())
            }
        }
    }

    pub(crate) fn of_state(state: &PluginPresetState) -> Self {
        match state {
            PluginPresetState::Effect { params, .. } => Processor::Effect(params.kind()),
            PluginPresetState::Instrument { params, .. } => Processor::Instrument(params.kind()),
            PluginPresetState::Hosted { binding, .. } => Self::hosted(binding),
        }
    }

    /// Whether automation of the setting at `param` of `self` still means
    /// the same setting once `next` has taken its place.
    pub(crate) fn keeps_param(&self, next: &Processor, param: u32) -> bool {
        match (self, next) {
            (Processor::Instrument(old), Processor::Instrument(new)) => old == new,
            (Processor::Effect(old), Processor::Effect(new)) => old == new,
            (
                Processor::Hosted {
                    format: old_format,
                    id: old_id,
                    parameters: old,
                },
                Processor::Hosted {
                    format: new_format,
                    id: new_id,
                    parameters: new,
                },
            ) => {
                old_format == new_format
                    && old_id == new_id
                    && match (old.get(param as usize), new.get(param as usize)) {
                        (Some((old, _)), Some((new, automatable))) => old == new && *automatable,
                        _ => false,
                    }
            }
            _ => false,
        }
    }
}

fn placeholder(instrument: bool) -> PluginTarget {
    if instrument {
        PluginTarget::Instrument {
            channel: ChannelId(0),
        }
    } else {
        PluginTarget::Effect {
            effect: EffectId(0),
        }
    }
}

fn name_problem(name: &str, what: &str) -> Option<String> {
    if name.len() > MAX_PRESET_NAME_BYTES || name.chars().any(char::is_control) {
        Some(format!(
            "The {what} name is longer than {MAX_PRESET_NAME_BYTES} bytes or has control characters"
        ))
    } else {
        None
    }
}

fn clean_name(name: &str) -> String {
    let mut name: String = name.chars().filter(|c| !c.is_control()).collect();
    while name.len() > MAX_PRESET_NAME_BYTES {
        name.pop();
    }
    name.trim().to_owned()
}

impl PluginPresetState {
    /// Whether this is the state of an instrument, as opposed to an effect.
    pub fn is_instrument(&self) -> bool {
        match self {
            PluginPresetState::Effect { .. } => false,
            PluginPresetState::Instrument { .. } => true,
            PluginPresetState::Hosted { instrument, .. } => *instrument,
        }
    }

    /// Which plugin the state belongs to.
    pub fn identity(&self) -> PresetPluginIdentity {
        let tag = |value: Result<Value, serde_json::Error>| {
            value
                .ok()
                .and_then(|value| value.get("type")?.as_str().map(str::to_owned))
                .unwrap_or_default()
        };
        match self {
            PluginPresetState::Effect { params, .. } => PresetPluginIdentity {
                format: "builtin".to_owned(),
                id: tag(serde_json::to_value(params)),
                instrument: false,
            },
            PluginPresetState::Instrument { params, .. } => PresetPluginIdentity {
                format: "builtin".to_owned(),
                id: tag(serde_json::to_value(params)),
                instrument: true,
            },
            PluginPresetState::Hosted {
                instrument,
                binding,
            } => PresetPluginIdentity {
                format: binding.format.clone(),
                id: binding.id.clone(),
                instrument: *instrument,
            },
        }
    }

    /// Whether `seen`, the state a destination had when a preset was chosen
    /// for it, still describes the destination that has this state.
    ///
    /// Built-in settings must be exactly as they were. A hosted plugin must
    /// be the same plugin from the same file: its native state can change
    /// on its own at any moment, so that is not compared.
    pub fn same_destination(&self, seen: &Self) -> bool {
        match (self, seen) {
            (
                PluginPresetState::Hosted {
                    instrument,
                    binding,
                },
                PluginPresetState::Hosted {
                    instrument: seen_instrument,
                    binding: seen,
                },
            ) => {
                instrument == seen_instrument
                    && binding.format == seen.format
                    && binding.id == seen.id
                    && binding.path == seen.path
            }
            (now, seen) => now == seen,
        }
    }

    fn validate(&self) -> Result<(), String> {
        match self {
            PluginPresetState::Effect { params, impulse } => {
                if let Some(asset) = impulse {
                    if params.kind() != EffectKind::Convolver {
                        return Err("Only a convolver preset can hold an IR".into());
                    }
                    asset.check().map_err(str::to_owned)?;
                }
                if params.sanitized() != *params {
                    return Err("The preset has effect settings outside their ranges".to_owned());
                }
            }
            PluginPresetState::Instrument { params, asset } => {
                if let Some(asset) = asset {
                    asset.validate(params.kind()).map_err(str::to_owned)?;
                }
                if params.sanitized() != *params {
                    return Err(
                        "The preset has instrument settings outside their ranges".to_owned()
                    );
                }
            }
            PluginPresetState::Hosted {
                instrument,
                binding,
            } => {
                binding.validate().map_err(str::to_owned)?;
                if binding.target != placeholder(*instrument) {
                    return Err("The preset's plugin names a project object".to_owned());
                }
            }
        }
        Ok(())
    }
}

impl PluginPreset {
    /// The state of the instrument or effect at `target`.
    ///
    /// A sampler channel has no plugin state: its sound is a channel
    /// preset. The native state of a hosted plugin is whatever the project
    /// holds for it, so capture the live state into the project first.
    pub fn capture(project: &Project, target: PluginTarget) -> Result<Self, String> {
        let binding = project.plugin(target).cloned().map(|mut binding| {
            binding.target = placeholder(matches!(target, PluginTarget::Instrument { .. }));
            binding
        });
        let state = match target {
            PluginTarget::Instrument { channel } => {
                let channel = project
                    .channel(channel)
                    .ok_or("The preset's channel no longer exists")?;
                let ChannelSource::Instrument { params, asset } = &channel.source else {
                    return Err(format!(
                        "\"{}\" is a sampler: save it as a channel preset",
                        channel.name
                    ));
                };
                match binding {
                    Some(binding) => PluginPresetState::Hosted {
                        instrument: true,
                        binding,
                    },
                    None => PluginPresetState::Instrument {
                        params: *params,
                        asset: asset.clone(),
                    },
                }
            }
            PluginTarget::Effect { effect } => {
                let slot = project
                    .mixer
                    .tracks
                    .iter()
                    .flat_map(|track| &track.effects)
                    .find(|slot| slot.id == effect)
                    .ok_or("The preset's effect no longer exists")?;
                match binding {
                    Some(binding) => PluginPresetState::Hosted {
                        instrument: false,
                        binding,
                    },
                    None => PluginPresetState::Effect {
                        params: slot.params,
                        impulse: slot.impulse.clone(),
                    },
                }
            }
        };
        let preset = Self { state };
        preset.validate()?;
        Ok(preset)
    }

    /// Says what stops this preset being loaded, if anything does.
    pub fn validate(&self) -> Result<(), String> {
        self.state.validate()
    }
}

impl ChannelPreset {
    /// What `id` sounds like, and the sample its sampler plays, whose audio
    /// the preset file has to bring along.
    pub fn capture(
        project: &Project,
        id: ChannelId,
    ) -> Result<(Self, Option<SampleAsset>), String> {
        let channel = project
            .channel(id)
            .ok_or("The preset's channel no longer exists")?;
        if channel.sample_playback.is_some() {
            return Err("Sample playback sources, maps and curves cannot be bundled in a channel preset yet. Save a project or duplicate the channel to retain them.".into());
        }
        if channel.sample_instrument.is_some() {
            return Err("Save this sample-bank instrument in its project; portable bank preset export is not supported yet".into());
        }
        let mut source = channel.source.clone();
        let mut sample = None;
        if let ChannelSource::Sampler(sampler) = &mut source {
            sample = sampler
                .sample
                .take()
                .and_then(|id| project.sample(id))
                .cloned();
        }
        let plugin = project
            .plugin(PluginTarget::Instrument { channel: id })
            .cloned()
            .map(|mut binding| {
                binding.target = placeholder(true);
                binding
            });
        let preset = Self {
            name: clean_name(&channel.name),
            color: channel.color,
            volume: channel.volume,
            pan: channel.pan,
            voice: channel.voice,
            timing: channel.timing,
            source,
            // The asset list is the caller's: it knows where the audio goes.
            sample_asset: sample.as_ref().map(|_| 1),
            plugin,
            rack: project
                .settings
                .modular_racks
                .iter()
                .find(|rack| rack.channel == id)
                .map(|rack| rack.graph.clone()),
        };
        preset.validate(sample.as_ref().map(|_| 1))?;
        Ok((preset, sample))
    }

    /// The channel `base` becomes when this preset is loaded onto it, apart
    /// from its name and color. `sample` is the project sample that holds
    /// the preset's audio.
    pub(crate) fn onto(&self, base: &Channel, sample: Option<SampleId>) -> Channel {
        let mut source = self.source.clone();
        if let ChannelSource::Sampler(sampler) = &mut source {
            sampler.sample = sample;
        }
        let sample_instrument = match (&base.source, &source) {
            (ChannelSource::Instrument { params: old, .. }, ChannelSource::Instrument { params: new, .. })
                if old.kind() == new.kind() => base.sample_instrument.clone(),
            _ => None,
        };
        Channel {
            sample_playback: if matches!(&source, ChannelSource::Instrument { params, .. }
                if crate::sample_playback::supported(params.kind()))
            {
                base.sample_playback.clone()
            } else {
                None
            },
            sample_instrument,
            volume: self.volume,
            pan: self.pan,
            voice: self.voice,
            timing: self.timing,
            source,
            ..base.clone()
        }
    }

    /// Says what stops this body being loaded, if anything does. `asset` is
    /// the id of the one asset the preset lists, if it lists one.
    ///
    /// Ranges are checked by loading the body onto a channel of an empty
    /// project and checking that project, so a preset is held to exactly
    /// what a project file is.
    pub fn validate(&self, asset: Option<u32>) -> Result<(), String> {
        if let Some(problem) = name_problem(&self.name, "channel") {
            return Err(problem);
        }
        if self.color > 0xFF_FFFF
            || !self.volume.is_finite()
            || !(0.0..=MAX_GAIN).contains(&self.volume)
            || !self.pan.is_finite()
            || !(-1.0..=1.0).contains(&self.pan)
        {
            return Err(
                "The preset has a channel color, volume or pan outside its range".to_owned(),
            );
        }
        match (&self.source, self.sample_asset, &self.plugin) {
            (ChannelSource::Sampler(sampler), wanted, None) => {
                if sampler.sample.is_some() {
                    return Err("The preset's sampler names a project sample".to_owned());
                }
                if wanted.is_some() && wanted != asset {
                    return Err(
                        "The preset's sampler plays audio the preset does not list".to_owned()
                    );
                }
            }
            (ChannelSource::Sampler(_), _, Some(_)) => {
                return Err("The preset has a plugin on a sampler channel".to_owned());
            }
            (ChannelSource::Instrument { .. }, Some(_), _) => {
                return Err("The preset has audio on an instrument channel".to_owned());
            }
            (ChannelSource::Instrument { params, .. }, None, plugin) => {
                if params.sanitized() != *params {
                    return Err(
                        "The preset has instrument settings outside their ranges".to_owned()
                    );
                }
                if let Some(binding) = plugin {
                    binding.validate().map_err(str::to_owned)?;
                    if binding.target != placeholder(true) {
                        return Err("The preset's plugin names a project object".to_owned());
                    }
                }
            }
        }
        self.probe()
    }

    /// Loads the body onto a channel of an empty project and checks it.
    fn probe(&self) -> Result<(), String> {
        let mut project = Project::new("Preset");
        let mut next = project.next_id;
        let mut take = || {
            let id = next;
            next += 1;
            id
        };
        let sample = self.sample_asset.map(|_| SampleId(take()));
        if let Some(id) = sample {
            project.samples.push(SampleAsset {
                id,
                name: "Preset audio".to_owned(),
                path: SamplePath::External("preset-audio".to_owned()),
            });
        }
        let id = ChannelId(take());
        let base = Channel {
            sample_playback: None,
            voice: ChannelVoiceSettings::default(),
            id,
            name: "Preset".to_owned(),
            group: String::new(),
            timing: ChannelTiming::default(),
            color: 0,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            mixer_track: TrackId::MASTER,
            source: self.source.clone(),
            expression_map: Default::default(),
            sample_instrument: None,
        };
        project.channels.push(self.onto(&base, sample));
        if let Some(graph) = &self.rack {
            project.settings.modular_racks.push(crate::ChannelRack {
                channel: id,
                graph: graph.clone(),
            });
        }
        if let Some(binding) = &self.plugin {
            let mut binding = binding.clone();
            binding.target = PluginTarget::Instrument { channel: id };
            project.plugins.push(binding);
        }
        project.next_id = next;
        project
            .check()
            .map_err(|problem| format!("The preset is not valid: {problem}"))
    }
}

/// Only what tells whether a file is a preset this build can read.
#[derive(Deserialize)]
struct Header {
    schema: Option<String>,
    version: Option<u32>,
}

impl Preset {
    fn new(kind: PresetKind, name: &str) -> Self {
        Self {
            schema: PRESET_SCHEMA.to_owned(),
            version: PRESET_VERSION,
            kind,
            name: clean_name(name),
            assets: Vec::new(),
            channel: None,
            plugin: None,
            mixer_track: None,
        }
    }

    /// A channel preset. `asset` is where the audio of its sampler is, when
    /// it has any; it gets the id the body refers to.
    pub fn of_channel(name: &str, mut body: ChannelPreset, asset: Option<PresetAsset>) -> Self {
        let mut preset = Self::new(PresetKind::Channel, name);
        body.sample_asset = asset.as_ref().map(|asset| asset.id);
        preset.assets.extend(asset);
        preset.channel = Some(body);
        preset
    }

    /// A plugin preset.
    pub fn of_plugin(name: &str, body: PluginPreset) -> Self {
        let mut preset = Self::new(PresetKind::Plugin, name);
        preset.plugin = Some(body);
        preset
    }

    /// A mixer track preset, named after the track.
    pub fn of_mixer_track(body: MixerTrackPreset) -> Self {
        let mut preset = Self::new(PresetKind::MixerTrack, &body.name);
        preset.mixer_track = Some(body);
        preset
    }

    /// Which plugin a plugin preset, or the instrument of a channel preset,
    /// belongs to. `None` for a sampler channel and for a mixer track.
    pub fn plugin_identity(&self) -> Option<PresetPluginIdentity> {
        if let Some(plugin) = &self.plugin {
            return Some(plugin.state.identity());
        }
        let channel = self.channel.as_ref()?;
        match (&channel.source, &channel.plugin) {
            (ChannelSource::Sampler(_), _) => None,
            (ChannelSource::Instrument { .. }, Some(binding)) => Some(PresetPluginIdentity {
                format: binding.format.clone(),
                id: binding.id.clone(),
                instrument: true,
            }),
            (ChannelSource::Instrument { params, .. }, None) => Some(
                PluginPresetState::Instrument {
                    params: *params,
                    asset: None,
                }
                .identity(),
            ),
        }
    }

    /// Says what stops this preset being loaded, if anything does.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PRESET_SCHEMA {
            return Err("This file is not a Windfall preset".to_owned());
        }
        if self.version == 0 {
            return Err("The preset has no version".to_owned());
        }
        if self.version > PRESET_VERSION {
            return Err(newer(self.version));
        }
        if let Some(problem) = name_problem(&self.name, "preset") {
            return Err(problem);
        }
        if self.assets.len() > MAX_PRESET_ASSETS {
            return Err(format!(
                "A preset lists at most {MAX_PRESET_ASSETS} audio files"
            ));
        }
        let mut ids = HashSet::new();
        let mut paths = HashSet::new();
        for asset in &self.assets {
            if !ids.insert(asset.id) || !paths.insert(asset.path.to_lowercase()) {
                return Err("The preset lists an audio file twice".to_owned());
            }
            if let Some(problem) = name_problem(&asset.name, "audio") {
                return Err(problem);
            }
            if let Some(problem) = relative_path_problem(&asset.path) {
                return Err(format!(
                    "The preset's audio path \"{}\" is not valid: {problem}",
                    asset.path
                ));
            }
            if asset.path.contains(':') || asset.path.starts_with('/') {
                return Err(format!(
                    "The preset's audio path \"{}\" is not valid: it must be relative",
                    asset.path
                ));
            }
            if asset.bytes > MAX_PRESET_ASSET_BYTES {
                return Err(format!(
                    "The preset's audio \"{}\" is larger than a preset can list",
                    asset.name
                ));
            }
        }
        let bodies = (
            self.channel.as_ref(),
            self.plugin.as_ref(),
            self.mixer_track.as_ref(),
        );
        match (self.kind, bodies) {
            (PresetKind::Channel, (Some(channel), None, None)) => {
                let asset = match self.assets.as_slice() {
                    [] => None,
                    [asset] => Some(asset.id),
                    _ => {
                        return Err("A channel preset lists at most one audio file".to_owned());
                    }
                };
                if asset.is_some() && channel.sample_asset != asset {
                    return Err("The preset lists audio that nothing in it plays".to_owned());
                }
                channel.validate(asset)
            }
            (PresetKind::Plugin, (None, Some(plugin), None)) if self.assets.is_empty() => {
                plugin.validate()
            }
            (PresetKind::MixerTrack, (None, None, Some(track))) if self.assets.is_empty() => {
                track.validate()
            }
            _ => Err(format!(
                "The file says it is a {} preset but does not hold exactly that",
                self.kind.label()
            )),
        }
    }

    /// The file a preset is stored as.
    pub fn to_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|error| error.to_string())
    }

    /// Reads a preset file, carrying the settings of built-in processors
    /// over from older builds. Returns the preset and one line for each
    /// thing that was changed on the way in.
    pub fn from_json(bytes: &[u8]) -> Result<(Self, Vec<String>), String> {
        Self::from_json_with(bytes, PARAM_RENAMES)
    }

    /// [`from_json`](Self::from_json) with a rename table of the caller's.
    pub fn from_json_with(
        bytes: &[u8],
        renames: &[ParamRename],
    ) -> Result<(Self, Vec<String>), String> {
        if bytes.len() > MAX_PRESET_FILE_BYTES {
            return Err("The preset file is larger than 256 MiB".to_owned());
        }
        let header: Header = serde_json::from_slice(bytes)
            .map_err(|error| format!("This file is not a Windfall preset: {error}"))?;
        if header.schema.as_deref() != Some(PRESET_SCHEMA) {
            return Err("This file is not a Windfall preset".to_owned());
        }
        match header.version {
            Some(version) if version > PRESET_VERSION => return Err(newer(version)),
            Some(0) | None => return Err("The preset has no version".to_owned()),
            Some(_) => {}
        }
        let mut value: Value = serde_json::from_slice(bytes)
            .map_err(|error| format!("The preset file is damaged: {error}"))?;
        let mut notes = Vec::new();
        migrate(&mut value, renames, &mut notes)?;
        let preset: Preset = serde_json::from_value(value)
            .map_err(|error| format!("The preset file is damaged: {error}"))?;
        preset.validate()?;
        Ok((preset, notes))
    }
}

fn newer(version: u32) -> String {
    format!(
        "This preset was saved by a newer Windfall (preset version {version}); this build reads up to version {PRESET_VERSION}"
    )
}

/// The default settings of every built-in effect, by the `type` a file
/// gives it.
fn effect_defaults() -> &'static HashMap<String, Value> {
    static TABLE: OnceLock<HashMap<String, Value>> = OnceLock::new();
    TABLE.get_or_init(|| {
        EffectKind::ALL
            .iter()
            .filter_map(|kind| tagged(serde_json::to_value(kind.default_params()).ok()?))
            .collect()
    })
}

fn instrument_defaults() -> &'static HashMap<String, Value> {
    static TABLE: OnceLock<HashMap<String, Value>> = OnceLock::new();
    TABLE.get_or_init(|| {
        InstrumentKind::ALL
            .iter()
            .filter_map(|kind| tagged(serde_json::to_value(kind.default_params()).ok()?))
            .collect()
    })
}

fn tagged(value: Value) -> Option<(String, Value)> {
    Some((value.get("type")?.as_str()?.to_owned(), value))
}

/// Brings the built-in settings in a preset's JSON to what this build
/// stores, wherever the three bodies keep them.
fn migrate(
    value: &mut Value,
    renames: &[ParamRename],
    notes: &mut Vec<String>,
) -> Result<(), String> {
    if let Some(source) = value.pointer_mut("/channel/source")
        && source.get("type").and_then(Value::as_str) == Some("instrument")
        && let Some(params) = source.get_mut("params")
    {
        migrate_params(params, instrument_defaults(), "instrument", renames, notes)?;
    }
    if let Some(state) = value.pointer_mut("/plugin/state") {
        crate::convolver::migrate_impulse_holder(state)?;
        let kind = state.get("type").and_then(Value::as_str).map(str::to_owned);
        if let Some(params) = state.get_mut("params") {
            match kind.as_deref() {
                Some("effect") => {
                    migrate_params(params, effect_defaults(), "effect", renames, notes)?
                }
                Some("instrument") => {
                    migrate_params(params, instrument_defaults(), "instrument", renames, notes)?
                }
                _ => {}
            }
        }
    }
    if let Some(effects) = value
        .pointer_mut("/mixerTrack/effects")
        .and_then(Value::as_array_mut)
    {
        for slot in effects {
            crate::convolver::migrate_impulse_holder(slot)?;
            if let Some(params) = slot.get_mut("params") {
                migrate_params(params, effect_defaults(), "effect", renames, notes)?;
            }
        }
    }
    Ok(())
}

/// Brings one processor's settings to what this build stores: renamed
/// settings move to their new names, unknown ones go, missing ones take
/// their defaults and values outside their range are brought into it.
fn migrate_params(
    params: &mut Value,
    defaults: &HashMap<String, Value>,
    what: &str,
    renames: &[ParamRename],
    notes: &mut Vec<String>,
) -> Result<(), String> {
    let Some(given) = params.as_object_mut() else {
        return Err(format!("The preset's {what} settings are not an object"));
    };
    let Some(tag) = given.get("type").and_then(Value::as_str).map(str::to_owned) else {
        return Err(format!(
            "The preset's {what} does not say which {what} it is"
        ));
    };
    let Some(default) = defaults.get(&tag).and_then(Value::as_object) else {
        return Err(format!(
            "The preset uses the {what} \"{tag}\", which this build of Windfall does not have"
        ));
    };
    for rename in renames.iter().filter(|rename| rename.processor == tag) {
        if let Some(old) = given.remove(rename.from) {
            if !given.contains_key(rename.to) {
                given.insert(rename.to.to_owned(), old);
            }
            notes.push(format!(
                "The {tag} setting \"{}\" is now called \"{}\"; its value was kept.",
                rename.from, rename.to
            ));
        }
    }
    let unknown: Vec<String> = given
        .keys()
        .filter(|key| {
            !default.contains_key(*key)
                && !(tag == "pitchCorrect" && key.as_str() == "harmonyChannel")
        })
        .cloned()
        .collect();
    for key in unknown {
        given.remove(&key);
        notes.push(format!(
            "The {tag} setting \"{key}\" is not one this build has and was left out."
        ));
    }
    let missing: Vec<&String> = default
        .keys()
        .filter(|key| !given.contains_key(*key))
        .collect();
    for key in missing {
        given.insert(key.clone(), default[key].clone());
        notes.push(format!(
            "The preset has no {tag} setting \"{key}\"; it starts at its default."
        ));
    }
    // Typed, so that ranges are applied the way the processor applies them.
    let damaged =
        |error: serde_json::Error| format!("The preset's {tag} settings are damaged: {error}");
    let (in_range, moved) = if what == "effect" {
        let typed: EffectParams = serde_json::from_value(params.clone()).map_err(damaged)?;
        let sanitized = typed.sanitized();
        (serde_json::to_value(sanitized), sanitized != typed)
    } else {
        let typed: InstrumentParams = serde_json::from_value(params.clone()).map_err(damaged)?;
        let sanitized = typed.sanitized();
        (serde_json::to_value(sanitized), sanitized != typed)
    };
    if moved {
        notes.push(format!(
            "Some {tag} settings were outside their ranges and were brought into them."
        ));
    }
    *params = in_range.map_err(|error| error.to_string())?;
    Ok(())
}
