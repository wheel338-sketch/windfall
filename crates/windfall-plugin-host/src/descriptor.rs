//! What the host knows about a plugin before and after loading it: who made
//! it, what it is, and which ports it has.
//!
//! These types are plain data. The scanner prints them as JSON, the cache
//! stores them, and the project file will name a plugin by its
//! [`PluginFormat`] and [`PluginDescriptor::id`].

use serde::{Deserialize, Serialize};

/// The plugin standard a file follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginFormat {
    Clap,
    Vst3,
    Au,
}

impl PluginFormat {
    /// The format of a file, judged by its extension.
    pub fn of(path: &std::path::Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?;
        if extension.eq_ignore_ascii_case("clap") {
            Some(Self::Clap)
        } else if extension.eq_ignore_ascii_case("vst3") {
            Some(Self::Vst3)
        } else {
            None
        }
    }
}

/// Where a plugin belongs in the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginKind {
    /// Makes sound from notes. It goes on a channel.
    Instrument,
    /// Changes sound. It goes in a mixer track's effect slot.
    Effect,
    /// Neither, such as a note effect or an analyzer the host has no place
    /// for yet.
    Other,
}

impl PluginKind {
    /// Reads the kind from a CLAP feature list or a VST3 subcategory list.
    /// A plugin that claims to be both is taken to be an instrument, since
    /// that is what a synth with an audio input says.
    pub fn from_features<'a>(features: impl IntoIterator<Item = &'a str>) -> Self {
        let mut kind = Self::Other;
        for feature in features {
            match feature.to_ascii_lowercase().as_str() {
                "instrument" | "synthesizer" | "sampler" | "drum" | "drum-machine" | "synth"
                | "piano" => return Self::Instrument,
                "audio-effect" | "fx" => kind = Self::Effect,
                _ => {}
            }
        }
        kind
    }
}

/// What a plugin file says about one of its plugins without creating it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDescriptor {
    pub format: PluginFormat,
    /// The plugin's own, permanent identifier. For CLAP this is its id
    /// string such as `com.u-he.diva`. For VST3 it is the class id as 32
    /// hexadecimal digits.
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    /// CLAP features, or the parts of a VST3 subcategory string.
    pub features: Vec<String>,
    pub kind: PluginKind,
}

/// One audio input or output of a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioPort {
    pub name: String,
    pub channels: u32,
    /// The port the track's own sound goes through. The others are
    /// sidechains and extra outputs.
    pub main: bool,
}

/// What a plugin says about itself once it exists.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginLayout {
    pub audio_inputs: Vec<AudioPort>,
    pub audio_outputs: Vec<AudioPort>,
    pub note_inputs: u32,
    pub note_outputs: u32,
    pub has_editor: bool,
    pub parameter_count: u32,
    pub has_state: bool,
}

/// The most ports, and the most channels in one port, the host accepts. A
/// plugin that reports more is refused, since the numbers can only be wrong.
pub const MAX_PORTS: u32 = 64;
/// See [`MAX_PORTS`].
pub const MAX_PORT_CHANNELS: u32 = 64;

/// The most parameters the host reads from one plugin. Large samplers have a
/// few thousand.
pub const MAX_PARAMETERS: u32 = 65_536;
