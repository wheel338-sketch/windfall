//! What a channel's stored per-note controls drive.
//!
//! Modulation X, modulation Y and release are stored on every note as values
//! from 0 to 1 with 0.5 neutral. A channel's [`ExpressionMap`] says which
//! voice control each of them moves, by how much and in which direction. The
//! default map is the fixed behaviour older projects were made with, and
//! resolving it changes nothing: not one bit of a note's controls.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::NoteExpression;

/// A control of a built-in voice: a sampler voice or a built-in instrument's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ExpressionDestination {
    /// The control does nothing.
    #[default]
    None,
    /// Filter cutoff, four octaves each way at full amount.
    Cutoff,
    /// Filter resonance, a factor of four each way at full amount.
    Resonance,
    /// Voice level, [`ExpressionMap::VOLUME_RANGE_DB`] each way at full amount.
    Volume,
    /// Added to the note's pan: hard left to hard right at full amount.
    Pan,
    /// Pitch, [`ExpressionMap::pitch_range_cents`] each way at full amount.
    Pitch,
    /// Envelope release time, a factor of eight each way at full amount.
    Release,
}

/// A standard per-note control of a hosted plugin. A plugin reports which of
/// them it takes; a destination it does not take is skipped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PluginExpressionDestination {
    #[default]
    None,
    Volume,
    Pan,
    Tuning,
    Vibrato,
    Expression,
    Brightness,
    Pressure,
}

impl PluginExpressionDestination {
    /// Every destination that is a plugin control, in the order of the
    /// plugin host's expression kinds and of [`PluginControls::values`].
    pub const KINDS: [Self; PLUGIN_EXPRESSION_KINDS] = [
        Self::Volume,
        Self::Pan,
        Self::Tuning,
        Self::Vibrato,
        Self::Expression,
        Self::Brightness,
        Self::Pressure,
    ];

    /// Position in [`PluginControls::values`]; `None` for no destination.
    pub const fn index(self) -> Option<usize> {
        match self {
            Self::None => None,
            Self::Volume => Some(0),
            Self::Pan => Some(1),
            Self::Tuning => Some(2),
            Self::Vibrato => Some(3),
            Self::Expression => Some(4),
            Self::Brightness => Some(5),
            Self::Pressure => Some(6),
        }
    }

    /// The bit of this destination in a provider's supported-kinds mask.
    pub const fn mask(self) -> u32 {
        match self.index() {
            Some(index) => 1 << index,
            None => 0,
        }
    }
}

/// Standard plugin expression kinds: volume, pan, tuning, vibrato,
/// expression, brightness and pressure.
pub const PLUGIN_EXPRESSION_KINDS: usize = 7;
const VOLUME: usize = 0;
const PAN: usize = 1;
const TUNING: usize = 2;

/// Where one stored note control goes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExpressionRoute {
    /// What it drives in a sampler or built-in instrument.
    pub destination: ExpressionDestination,
    /// What it drives in a hosted plugin.
    pub plugin: PluginExpressionDestination,
    /// Depth, 0 to 1. At 1 the control covers the destination's whole range.
    pub amount: f32,
    /// Turns the control upside down: a high value lowers the destination.
    pub invert: bool,
}

impl ExpressionRoute {
    const fn new(destination: ExpressionDestination, plugin: PluginExpressionDestination) -> Self {
        Self {
            destination,
            plugin,
            amount: 1.0,
            invert: false,
        }
    }

    pub fn valid(&self) -> bool {
        self.amount.is_finite() && (0.0..=1.0).contains(&self.amount)
    }

    fn sanitized(self) -> Self {
        Self {
            amount: if self.amount.is_finite() {
                self.amount.clamp(0.0, 1.0)
            } else {
                1.0
            },
            ..self
        }
    }

    /// Full depth, not inverted: the stored value passes through untouched.
    fn passes(&self) -> bool {
        self.amount == 1.0 && !self.invert
    }

    fn depth(&self) -> f32 {
        if self.invert { -self.amount } else { self.amount }
    }

    /// The control as a signed offset from neutral, -0.5 to 0.5.
    fn offset(&self, value: f32) -> f32 {
        (unit(value) - 0.5) * self.depth()
    }
}

fn unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.5
    }
}

/// One channel's routing of modulation X, modulation Y and release.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ExpressionMap {
    /// Default: filter cutoff in built-in voices, brightness in plugins.
    pub modulation_x: ExpressionRoute,
    /// Default: filter resonance in built-in voices, expression in plugins.
    pub modulation_y: ExpressionRoute,
    /// Default: envelope release time in built-in voices. A hosted plugin
    /// always receives release as its note-off velocity; `plugin` can send
    /// it as an expression as well and is `None` by default.
    pub release: ExpressionRoute,
    /// Cents each way a control mapped to pitch covers at full amount.
    /// 1 to 4800, default 1200. Plugin tuning uses the same range.
    pub pitch_range_cents: f32,
    /// Semitones each way of the channel pitch bend sent to a plugin input
    /// that takes MIDI but no per-note pitch. It has to match the bend range
    /// set in the plugin. 1 to 96, default 2, the usual synth default.
    pub bend_range_semitones: f32,
}

impl Default for ExpressionMap {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A note's controls as a built-in voice takes them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceControls {
    /// `release`, `modulation_x` and `modulation_y` hold what drives the
    /// envelope release, the filter cutoff and the filter resonance. The
    /// other fields are the note's own.
    pub expression: NoteExpression,
    /// The note's pan with any mapped offset, -1 to 1.
    pub pan: f32,
    /// Linear gain from controls mapped to volume. 1 when there are none.
    pub gain: f32,
    /// Cents from controls mapped to pitch, on top of the note's fine pitch.
    pub pitch_cents: f32,
}

/// A note's controls as standard plugin expressions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PluginControls {
    /// By [`PluginExpressionDestination::index`]. Volume is a linear gain
    /// from 0 to 4, tuning is semitones from the note's key, and the rest run
    /// from 0 to 1 with pan centred on 0.5.
    pub values: [f64; PLUGIN_EXPRESSION_KINDS],
    /// The kinds the map drives. A kind outside it is never sent.
    pub mask: u32,
}

impl ExpressionMap {
    /// Decibels each way a control mapped to volume covers at full amount.
    pub const VOLUME_RANGE_DB: f32 = 24.0;
    pub const DEFAULT_PITCH_RANGE_CENTS: f32 = 1200.0;
    pub const DEFAULT_BEND_RANGE_SEMITONES: f32 = 2.0;
    pub const DEFAULT: Self = Self {
        modulation_x: ExpressionRoute::new(
            ExpressionDestination::Cutoff,
            PluginExpressionDestination::Brightness,
        ),
        modulation_y: ExpressionRoute::new(
            ExpressionDestination::Resonance,
            PluginExpressionDestination::Expression,
        ),
        release: ExpressionRoute::new(
            ExpressionDestination::Release,
            PluginExpressionDestination::None,
        ),
        pitch_range_cents: Self::DEFAULT_PITCH_RANGE_CENTS,
        bend_range_semitones: Self::DEFAULT_BEND_RANGE_SEMITONES,
    };

    pub fn is_default(&self) -> bool {
        *self == Self::DEFAULT
    }

    pub fn valid(&self) -> bool {
        self.modulation_x.valid()
            && self.modulation_y.valid()
            && self.release.valid()
            && self.pitch_range_cents.is_finite()
            && (1.0..=4800.0).contains(&self.pitch_range_cents)
            && self.bend_range_semitones.is_finite()
            && (1.0..=96.0).contains(&self.bend_range_semitones)
    }

    /// The map with every number forced into its range.
    pub fn sanitized(self) -> Self {
        let within = |value: f32, low: f32, high: f32, fallback: f32| {
            if value.is_finite() {
                value.clamp(low, high)
            } else {
                fallback
            }
        };
        Self {
            modulation_x: self.modulation_x.sanitized(),
            modulation_y: self.modulation_y.sanitized(),
            release: self.release.sanitized(),
            pitch_range_cents: within(
                self.pitch_range_cents,
                1.0,
                4800.0,
                Self::DEFAULT_PITCH_RANGE_CENTS,
            ),
            bend_range_semitones: within(
                self.bend_range_semitones,
                1.0,
                96.0,
                Self::DEFAULT_BEND_RANGE_SEMITONES,
            ),
        }
    }

    fn routes(&self, expression: &NoteExpression) -> [(ExpressionRoute, f32); 3] {
        [
            (self.modulation_x, expression.modulation_x),
            (self.modulation_y, expression.modulation_y),
            (self.release, expression.release),
        ]
    }

    /// True when the built-in destinations are the ones of the default map.
    fn builtin_default(&self) -> bool {
        let same = |route: &ExpressionRoute, default: &ExpressionRoute| {
            route.destination == default.destination && route.passes()
        };
        same(&self.modulation_x, &Self::DEFAULT.modulation_x)
            && same(&self.modulation_y, &Self::DEFAULT.modulation_y)
            && same(&self.release, &Self::DEFAULT.release)
    }

    /// The controls of a built-in voice for a note's pan and expression.
    /// Plain arithmetic on copies: safe on the audio thread.
    pub fn resolve(&self, pan: f32, expression: NoteExpression) -> VoiceControls {
        if self.builtin_default() {
            return VoiceControls {
                expression,
                pan,
                gain: 1.0,
                pitch_cents: 0.0,
            };
        }
        let (mut cutoff, mut resonance, mut release) = (0.0_f32, 0.0_f32, 0.0_f32);
        let (mut volume, mut pan_offset, mut pitch) = (0.0_f32, 0.0_f32, 0.0_f32);
        for (route, value) in self.routes(&expression) {
            let offset = route.offset(value);
            match route.destination {
                ExpressionDestination::None => {}
                ExpressionDestination::Cutoff => cutoff += offset,
                ExpressionDestination::Resonance => resonance += offset,
                ExpressionDestination::Volume => volume += offset,
                ExpressionDestination::Pan => pan_offset += offset,
                ExpressionDestination::Pitch => pitch += offset,
                ExpressionDestination::Release => release += offset,
            }
        }
        let driver = |offset: f32| (0.5 + offset).clamp(0.0, 1.0);
        let pan = if pan.is_finite() { pan } else { 0.0 };
        VoiceControls {
            expression: NoteExpression {
                release: driver(release),
                modulation_x: driver(cutoff),
                modulation_y: driver(resonance),
                ..expression
            },
            pan: if pan_offset == 0.0 {
                pan
            } else {
                (pan + pan_offset * 2.0).clamp(-1.0, 1.0)
            },
            gain: if volume == 0.0 {
                1.0
            } else {
                // Decibels to gain: 10^(dB / 20) is 2^(dB / 6.0206).
                (volume.clamp(-0.5, 0.5) * 2.0 * Self::VOLUME_RANGE_DB
                    / (20.0 * std::f32::consts::LOG10_2))
                    .exp2()
            },
            pitch_cents: pitch.clamp(-0.5, 0.5) * 2.0 * self.pitch_range_cents,
        }
    }

    /// The standard expressions of a hosted plugin for a note. `tuning` is
    /// the note's pitch in semitones from its key, before mapped controls.
    /// Pan and tuning are always driven; the other kinds only when a control
    /// is mapped to them.
    pub fn plugin_controls(
        &self,
        pan: f32,
        tuning: f64,
        expression: NoteExpression,
    ) -> PluginControls {
        let mut values = [0.5_f64; PLUGIN_EXPRESSION_KINDS];
        let mut offsets = [0.0_f64; PLUGIN_EXPRESSION_KINDS];
        let mut sources = [0_u8; PLUGIN_EXPRESSION_KINDS];
        let mut exact = [true; PLUGIN_EXPRESSION_KINDS];
        let mut mask = (1 << PAN) | (1 << TUNING);
        for (route, value) in self.routes(&expression) {
            let Some(kind) = route.plugin.index() else {
                continue;
            };
            mask |= 1 << kind;
            sources[kind] += 1;
            offsets[kind] += f64::from(route.offset(value));
            exact[kind] &= route.passes();
            values[kind] = f64::from(unit(value));
        }
        for kind in 0..PLUGIN_EXPRESSION_KINDS {
            // One full-depth control reaches the plugin exactly as stored.
            if sources[kind] != 1 || !exact[kind] {
                values[kind] = (0.5 + offsets[kind]).clamp(0.0, 1.0);
            }
        }
        let pan = if pan.is_finite() { f64::from(pan) } else { 0.0 };
        values[PAN] = if mask_has(self, PluginExpressionDestination::Pan) {
            ((pan + 1.0) / 2.0 + offsets[PAN]).clamp(0.0, 1.0)
        } else {
            (pan + 1.0) / 2.0
        };
        values[TUNING] = if mask_has(self, PluginExpressionDestination::Tuning) {
            (tuning + offsets[TUNING] * 2.0 * f64::from(self.pitch_range_cents) / 100.0)
                .clamp(-120.0, 120.0)
        } else {
            tuning
        };
        values[VOLUME] = if mask_has(self, PluginExpressionDestination::Volume) {
            (offsets[VOLUME].clamp(-0.5, 0.5) * 2.0 * f64::from(Self::VOLUME_RANGE_DB)
                / (20.0 * std::f64::consts::LOG10_2))
                .exp2()
                .clamp(0.0, 4.0)
        } else {
            1.0
        };
        PluginControls { values, mask }
    }

    /// The plugin kinds this map drives besides pan and tuning.
    pub fn plugin_mask(&self) -> u32 {
        self.modulation_x.plugin.mask() | self.modulation_y.plugin.mask() | self.release.plugin.mask()
    }
}

fn mask_has(map: &ExpressionMap, destination: PluginExpressionDestination) -> bool {
    map.plugin_mask() & destination.mask() != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(x: f32, y: f32, release: f32) -> NoteExpression {
        NoteExpression {
            modulation_x: x,
            modulation_y: y,
            release,
            fine_pitch_cents: 12.5,
            ..NoteExpression::NEUTRAL
        }
    }

    #[test]
    fn the_default_map_returns_every_note_control_bit_for_bit() {
        let map = ExpressionMap::default();
        assert!(map.is_default() && map.valid());
        for (x, y, release, pan) in [
            (0.0, 1.0, 0.5, -1.0),
            (0.1, 0.123_456_7, 0.999_999_9, 0.333_333_34),
            (f32::MIN_POSITIVE, 0.75, 0.2, 0.0),
            (1.0e-7, 0.249_999_99, 0.500_000_06, -0.0),
        ] {
            let expression = note(x, y, release);
            let controls = map.resolve(pan, expression);
            assert_eq!(controls.expression, expression);
            assert_eq!(controls.pan.to_bits(), pan.to_bits());
            assert_eq!(controls.gain.to_bits(), 1.0_f32.to_bits());
            assert_eq!(controls.pitch_cents.to_bits(), 0.0_f32.to_bits());
            let plugin = map.plugin_controls(pan, 0.125, expression);
            assert_eq!(plugin.mask, 0b0110110, "pan, tuning, expression, brightness");
            assert_eq!(plugin.values[1], (f64::from(pan) + 1.0) / 2.0);
            assert_eq!(plugin.values[2], 0.125);
            assert_eq!(plugin.values[5], f64::from(x), "modulation X is brightness");
            assert_eq!(plugin.values[4], f64::from(y), "modulation Y is expression");
        }
    }

    #[test]
    fn each_built_in_destination_moves_only_its_own_control() {
        let expression = note(1.0, 0.5, 0.5);
        let with = |destination| ExpressionMap {
            modulation_x: ExpressionRoute {
                destination,
                ..ExpressionMap::DEFAULT.modulation_x
            },
            ..ExpressionMap::DEFAULT
        };
        let neutral = with(ExpressionDestination::None).resolve(0.25, expression);
        assert_eq!(neutral.expression.modulation_x, 0.5);
        assert_eq!((neutral.pan, neutral.gain, neutral.pitch_cents), (0.25, 1.0, 0.0));
        let volume = with(ExpressionDestination::Volume).resolve(0.25, expression);
        assert_eq!(volume.expression.modulation_x, 0.5, "the filter stays where it was");
        assert!((20.0 * volume.gain.log10() - 24.0).abs() < 1.0e-3);
        let pan = with(ExpressionDestination::Pan).resolve(0.25, expression);
        assert_eq!((pan.pan, pan.gain), (1.0, 1.0));
        let pitch = with(ExpressionDestination::Pitch).resolve(0.25, expression);
        assert_eq!(pitch.pitch_cents, 1200.0);
        assert_eq!(pitch.expression.fine_pitch_cents, 12.5, "fine pitch is the note's own");
        let resonance = with(ExpressionDestination::Resonance).resolve(0.25, expression);
        assert_eq!(resonance.expression.modulation_y, 1.0);
        assert_eq!(resonance.expression.modulation_x, 0.5);
        let release = with(ExpressionDestination::Release).resolve(0.25, expression);
        assert_eq!(release.expression.release, 1.0);
    }

    #[test]
    fn amount_scales_and_invert_mirrors_a_route() {
        let map = ExpressionMap {
            modulation_x: ExpressionRoute {
                destination: ExpressionDestination::Pitch,
                plugin: PluginExpressionDestination::Tuning,
                amount: 0.5,
                invert: true,
            },
            pitch_range_cents: 200.0,
            ..ExpressionMap::DEFAULT
        };
        assert!(map.valid());
        let controls = map.resolve(0.0, note(1.0, 0.5, 0.5));
        assert_eq!(controls.pitch_cents, -100.0);
        let plugin = map.plugin_controls(0.0, 0.25, note(1.0, 0.5, 0.5));
        assert_eq!(plugin.values[2], 0.25 - 1.0, "half of two semitones down");
        assert_eq!(plugin.mask & PluginExpressionDestination::Brightness.mask(), 0);
        let both = ExpressionMap {
            modulation_y: ExpressionRoute {
                destination: ExpressionDestination::Cutoff,
                ..ExpressionMap::DEFAULT.modulation_y
            },
            ..ExpressionMap::DEFAULT
        };
        let controls = both.resolve(0.0, note(0.75, 0.75, 0.5));
        assert_eq!(controls.expression.modulation_x, 1.0, "two controls add up");
        assert_eq!(controls.expression.modulation_y, 0.5);
    }

    #[test]
    fn plugin_volume_and_pan_follow_the_standard_ranges() {
        let map = ExpressionMap {
            modulation_x: ExpressionRoute {
                plugin: PluginExpressionDestination::Volume,
                ..ExpressionMap::DEFAULT.modulation_x
            },
            modulation_y: ExpressionRoute {
                plugin: PluginExpressionDestination::Pan,
                ..ExpressionMap::DEFAULT.modulation_y
            },
            ..ExpressionMap::DEFAULT
        };
        let quiet = map.plugin_controls(0.0, 0.0, note(0.5, 0.5, 0.5));
        assert_eq!(quiet.values[0], 1.0, "neutral is unity gain");
        assert_eq!(quiet.values[1], 0.5);
        let loud = map.plugin_controls(0.0, 0.0, note(1.0, 1.0, 0.5));
        assert_eq!(loud.values[0], 4.0, "24 dB up is past the standard ceiling of 4");
        assert_eq!(loud.values[1], 1.0);
        assert_eq!(loud.mask, 0b0000111);
    }

    #[test]
    fn invalid_numbers_are_refused_and_sanitized() {
        let mut map = ExpressionMap::default();
        map.modulation_x.amount = 1.5;
        map.pitch_range_cents = f32::NAN;
        map.bend_range_semitones = 0.0;
        assert!(!map.valid());
        let clean = map.sanitized();
        assert!(clean.valid());
        assert_eq!(clean.modulation_x.amount, 1.0);
        assert_eq!(clean.pitch_range_cents, 1200.0);
        assert_eq!(clean.bend_range_semitones, 1.0);
    }

    #[test]
    fn a_map_written_without_newer_fields_takes_the_defaults() {
        let map: ExpressionMap = serde_json::from_str(
            r#"{"modulationX":{"destination":"volume","plugin":"pressure","amount":0.25,"invert":true}}"#,
        )
        .unwrap();
        assert_eq!(map.modulation_x.destination, ExpressionDestination::Volume);
        assert_eq!(map.modulation_y, ExpressionMap::DEFAULT.modulation_y);
        assert_eq!(map.bend_range_semitones, 2.0);
        assert_eq!(
            serde_json::from_str::<ExpressionMap>("{}").unwrap(),
            ExpressionMap::DEFAULT
        );
    }
}
