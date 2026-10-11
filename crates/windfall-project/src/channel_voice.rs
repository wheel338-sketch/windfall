//! Persistent channel voice settings; processors live in the engine.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_dsp::EnvelopeParams;

fn finite(value: f32, low: f32, high: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(low, high)
    } else {
        fallback
    }
}
fn sample_rate(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(1.0, 384_000.0)
    } else {
        48_000.0
    }
}
fn tempo(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(1.0, 999.0)
    } else {
        120.0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ChannelNoteDivision")]
pub enum NoteDivision {
    Quarter,
    Eighth,
    #[default]
    Sixteenth,
    ThirtySecond,
}

impl NoteDivision {
    pub fn beats(self) -> f64 {
        match self {
            Self::Quarter => 1.0,
            Self::Eighth => 0.5,
            Self::Sixteenth => 0.25,
            Self::ThirtySecond => 0.125,
        }
    }
    pub fn frames(self, rate: f64, bpm: f64) -> f64 {
        (sample_rate(rate) * 60.0 / tempo(bpm) * self.beats()).max(1.0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ArpeggiatorMode {
    #[default]
    Off,
    Up,
    Down,
    UpDown,
    AsPlayed,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ArpeggiatorSettings {
    pub mode: ArpeggiatorMode,
    pub rate: NoteDivision,
    pub gate: f32,
    pub range_octaves: u8,
}
impl Default for ArpeggiatorSettings {
    fn default() -> Self {
        Self {
            mode: ArpeggiatorMode::Off,
            rate: NoteDivision::Sixteenth,
            gate: 0.75,
            range_octaves: 1,
        }
    }
}
impl ArpeggiatorSettings {
    pub fn sanitized(self) -> Self {
        Self {
            gate: finite(self.gate, 0.0, 1.0, 0.75),
            range_octaves: self.range_octaves.clamp(1, 4),
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "unit", rename_all = "camelCase")]
#[ts(export)]
pub enum EchoTime {
    Milliseconds { ms: f32 },
    Division { division: NoteDivision },
}
impl Default for EchoTime {
    fn default() -> Self {
        Self::Milliseconds { ms: 250.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct NoteEchoSettings {
    pub enabled: bool,
    pub time: EchoTime,
    pub feedback: f32,
    pub pitch_semitones: i8,
    pub repeats: u8,
}
impl Default for NoteEchoSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            time: EchoTime::default(),
            feedback: 0.5,
            pitch_semitones: 0,
            repeats: 3,
        }
    }
}
impl NoteEchoSettings {
    pub fn sanitized(self) -> Self {
        Self {
            time: match self.time {
                EchoTime::Milliseconds { ms } => EchoTime::Milliseconds {
                    ms: finite(ms, 1.0, 60_000.0, 250.0),
                },
                time => time,
            },
            feedback: finite(self.feedback, 0.0, 0.95, 0.5),
            pitch_semitones: self.pitch_semitones.clamp(-48, 48),
            repeats: self.repeats.min(8),
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PolyphonySettings {
    pub max_voices: u8,
    pub mono_legato: bool,
    pub portamento_ms: f32,
}
impl Default for PolyphonySettings {
    fn default() -> Self {
        Self {
            max_voices: 32,
            mono_legato: false,
            portamento_ms: 0.0,
        }
    }
}
impl PolyphonySettings {
    pub fn sanitized(self) -> Self {
        Self {
            max_voices: self.max_voices.clamp(1, 32),
            portamento_ms: finite(self.portamento_ms, 0.0, 60_000.0, 0.0),
            ..self
        }
    }
    /// Add this fraction of `(target - current)` per sample. Time is the
    /// exponential time constant (63.2% covered); zero snaps to the target.
    pub fn glide_coefficient(self, rate: f64) -> f64 {
        let ms = f64::from(self.sanitized().portamento_ms);
        if ms == 0.0 {
            1.0
        } else {
            -(-1000.0 / (ms * sample_rate(rate))).exp_m1()
        }
    }
}

/// ADSR fields/defaults match synth::EnvelopeParams. Depth is filter octaves,
/// pitch cents, or normalized pan according to the enclosing field.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ModulationEnvelope {
    pub enabled: bool,
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
    pub depth: f32,
}
impl Default for ModulationEnvelope {
    fn default() -> Self {
        let shape = EnvelopeParams::default();
        Self {
            enabled: false,
            attack_ms: shape.attack_ms,
            decay_ms: shape.decay_ms,
            sustain: shape.sustain,
            release_ms: shape.release_ms,
            depth: 0.0,
        }
    }
}
impl ModulationEnvelope {
    fn sanitized(self, max_depth: f32) -> Self {
        let defaults = Self::default();
        Self {
            attack_ms: finite(self.attack_ms, 0.0, 10_000.0, defaults.attack_ms),
            decay_ms: finite(self.decay_ms, 1.0, 10_000.0, defaults.decay_ms),
            sustain: finite(self.sustain, 0.0, 1.0, defaults.sustain),
            release_ms: finite(self.release_ms, 1.0, 10_000.0, defaults.release_ms),
            depth: finite(self.depth, -max_depth, max_depth, 0.0),
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
#[ts(rename = "ChannelLfoShape")]
pub enum LfoShape {
    #[default]
    Sine,
    Triangle,
    Square,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LfoTarget {
    #[default]
    Filter,
    Pitch,
    Pan,
    /// Tremolo. Appended after the original three targets.
    Volume,
    /// Filter resonance, the second modulation target.
    Resonance,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ChannelLfoSettings {
    pub enabled: bool,
    pub shape: LfoShape,
    pub target: LfoTarget,
    pub rate_hz: f32,
    /// Normalized bipolar amount. Full depth is 8 octaves / 2400 cents / 1 pan.
    pub depth: f32,
}
impl Default for ChannelLfoSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            shape: LfoShape::Sine,
            target: LfoTarget::Filter,
            rate_hz: 5.0,
            depth: 0.0,
        }
    }
}
impl ChannelLfoSettings {
    pub fn sanitized(self) -> Self {
        Self {
            rate_hz: finite(self.rate_hz, 0.01, 30.0, 5.0),
            depth: finite(self.depth, -1.0, 1.0, 0.0),
            ..self
        }
    }
}

/// One LFO that always drives the target named by its field.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct TargetLfo {
    pub enabled: bool,
    pub shape: LfoShape,
    pub rate_hz: f32,
    /// Normalized bipolar amount; see [`ChannelTargetLfos`] for full scale.
    pub depth: f32,
}
impl Default for TargetLfo {
    fn default() -> Self {
        Self {
            enabled: false,
            shape: LfoShape::Sine,
            rate_hz: 5.0,
            depth: 0.0,
        }
    }
}
impl TargetLfo {
    pub fn sanitized(self) -> Self {
        Self {
            rate_hz: finite(self.rate_hz, 0.01, 30.0, 5.0),
            depth: finite(self.depth, -1.0, 1.0, 0.0),
            ..self
        }
    }
}

/// An independent LFO for each sampler modulation target. Full depth is a
/// tremolo down to silence, full pan travel, 2400 cents, 8 cutoff octaves
/// and the whole resonance range. They add to the assignable `lfo`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ChannelTargetLfos {
    pub volume: TargetLfo,
    pub pan: TargetLfo,
    pub pitch: TargetLfo,
    pub cutoff: TargetLfo,
    pub resonance: TargetLfo,
}
impl ChannelTargetLfos {
    pub fn sanitized(self) -> Self {
        Self {
            volume: self.volume.sanitized(),
            pan: self.pan.sanitized(),
            pitch: self.pitch.sanitized(),
            cutoff: self.cutoff.sanitized(),
            resonance: self.resonance.sanitized(),
        }
    }
    pub fn any_enabled(self) -> bool {
        self.volume.enabled
            || self.pan.enabled
            || self.pitch.enabled
            || self.cutoff.enabled
            || self.resonance.enabled
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum VoiceFilterMode {
    #[default]
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

pub const MIN_VOICE_CUTOFF_HZ: f32 = 20.0;
pub const MAX_VOICE_CUTOFF_HZ: f32 = 20_000.0;
/// Octaves of Q added by full resonance: Q runs from 0.707 to about 22.6.
pub const VOICE_RESONANCE_OCTAVES: f32 = 5.0;

/// The sampler channel's own filter. Cutoff is modulation target X and
/// resonance is target Y. Off leaves the sample unfiltered unless a cutoff
/// or resonance modulation is active, as before this setting existed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct VoiceFilter {
    pub enabled: bool,
    pub mode: VoiceFilterMode,
    pub cutoff_hz: f32,
    /// 0 has no resonant peak; 1 is the sharpest peak.
    pub resonance: f32,
}
impl Default for VoiceFilter {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: VoiceFilterMode::LowPass,
            cutoff_hz: 2000.0,
            resonance: 0.0,
        }
    }
}
impl VoiceFilter {
    pub fn sanitized(self) -> Self {
        Self {
            cutoff_hz: finite(
                self.cutoff_hz,
                MIN_VOICE_CUTOFF_HZ,
                MAX_VOICE_CUTOFF_HZ,
                2000.0,
            ),
            resonance: finite(self.resonance, 0.0, 1.0, 0.0),
            ..self
        }
    }
    /// Filter Q for a resonance amount, clamped to the supported range.
    pub fn q(resonance: f32) -> f32 {
        std::f32::consts::FRAC_1_SQRT_2
            * (finite(resonance, 0.0, 1.0, 0.0) * VOICE_RESONANCE_OCTAVES).exp2()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ChannelEnvelopes {
    /// Cutoff envelope (modulation target X), in octaves.
    pub filter: ModulationEnvelope,
    pub pitch: ModulationEnvelope,
    pub pan: ModulationEnvelope,
    /// One LFO that can be pointed at any target.
    pub lfo: ChannelLfoSettings,
    /// Resonance envelope (modulation target Y); depth is a resonance amount.
    /// Like the two fields after it, absent in older files and written only
    /// when it is not the default, so those files save back unchanged.
    #[serde(skip_serializing_if = "is_default")]
    #[ts(as = "Option<ModulationEnvelope>", optional)]
    pub resonance: ModulationEnvelope,
    /// A dedicated LFO per target.
    #[serde(skip_serializing_if = "is_default")]
    #[ts(as = "Option<ChannelTargetLfos>", optional)]
    pub lfos: ChannelTargetLfos,
    /// The channel filter that the cutoff and resonance targets move.
    #[serde(skip_serializing_if = "is_default")]
    #[ts(as = "Option<VoiceFilter>", optional)]
    pub filter_settings: VoiceFilter,
}
fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}
impl ChannelEnvelopes {
    pub fn sanitized(self) -> Self {
        Self {
            filter: self.filter.sanitized(8.0),
            pitch: self.pitch.sanitized(2400.0),
            pan: self.pan.sanitized(1.0),
            lfo: self.lfo.sanitized(),
            resonance: self.resonance.sanitized(1.0),
            lfos: self.lfos.sanitized(),
            filter_settings: self.filter_settings.sanitized(),
        }
    }
}

/// The most child channels one layer can play. A bound, so fanning a note
/// out on the audio thread is a fixed amount of work.
pub const MAX_LAYER_CHILDREN: usize = 16;

/// The channels a layer passes its notes to, in order, without repeats.
/// Stored inline so the enclosing settings stay `Copy`; written as a plain
/// list of channel ids.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, TS)]
#[ts(export, as = "Vec<crate::ChannelId>")]
pub struct LayerChildren {
    ids: [u32; MAX_LAYER_CHILDREN],
    len: u8,
}
impl LayerChildren {
    /// Keeps the first [`MAX_LAYER_CHILDREN`] distinct ids in order.
    pub fn new(children: impl IntoIterator<Item = crate::ChannelId>) -> Self {
        let mut out = Self::default();
        for child in children {
            out.push(child);
        }
        out
    }
    /// False when the list is full or already holds the channel.
    pub fn push(&mut self, child: crate::ChannelId) -> bool {
        let len = usize::from(self.len);
        if len == MAX_LAYER_CHILDREN || self.contains(child) {
            return false;
        }
        self.ids[len] = child.0;
        self.len += 1;
        true
    }
    pub fn remove(&mut self, child: crate::ChannelId) -> bool {
        let len = usize::from(self.len);
        let Some(index) = self.ids[..len].iter().position(|&id| id == child.0) else {
            return false;
        };
        self.ids.copy_within(index + 1..len, index);
        self.ids[len - 1] = 0;
        self.len -= 1;
        true
    }
    pub fn contains(&self, child: crate::ChannelId) -> bool {
        self.ids[..usize::from(self.len)].contains(&child.0)
    }
    pub fn len(&self) -> usize {
        usize::from(self.len)
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn iter(&self) -> impl Iterator<Item = crate::ChannelId> + '_ {
        self.ids[..usize::from(self.len)]
            .iter()
            .map(|&id| crate::ChannelId(id))
    }
}
impl Serialize for LayerChildren {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}
impl<'de> Deserialize<'de> for LayerChildren {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let children = Vec::<crate::ChannelId>::deserialize(de)?;
        if children.len() > MAX_LAYER_CHILDREN {
            return Err(serde::de::Error::custom(format!(
                "a layer plays at most {MAX_LAYER_CHILDREN} channels"
            )));
        }
        Ok(Self::new(children))
    }
}

/// Makes the channel a layer: it makes no sound of its own and plays every
/// note it is given on each child channel instead. Checked against the
/// project by [`crate::layer`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct LayerSettings {
    pub enabled: bool,
    pub children: LayerChildren,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ChannelVoiceSettings {
    pub arpeggiator: ArpeggiatorSettings,
    pub echo: NoteEchoSettings,
    pub polyphony: PolyphonySettings,
    pub envelopes: ChannelEnvelopes,
    /// Absent in older files and on every channel that is not a layer.
    #[serde(skip_serializing_if = "is_default")]
    #[ts(as = "Option<LayerSettings>", optional)]
    pub layer: LayerSettings,
}
impl ChannelVoiceSettings {
    pub fn sanitized(self) -> Self {
        Self {
            arpeggiator: self.arpeggiator.sanitized(),
            echo: self.echo.sanitized(),
            polyphony: self.polyphony.sanitized(),
            envelopes: self.envelopes.sanitized(),
            layer: self.layer,
        }
    }
}

pub(crate) fn deserialize_settings<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> Result<ChannelVoiceSettings, D::Error> {
    ChannelVoiceSettings::deserialize(de).map(ChannelVoiceSettings::sanitized)
}
