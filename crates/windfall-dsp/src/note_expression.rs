//! Per-note controls shared by samplers and built-in instruments.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Runtime identity scoped to one instrument owner, never a saved project
/// note id. Every loop pass, clip occurrence and live trigger gets a new id.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct NoteInstanceId(pub u64);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NoteArticulation {
    #[default]
    Normal,
    Slide,
    Portamento,
}

/// Neutral values preserve legacy playback. Modulation X controls filter
/// cutoff and modulation Y controls resonance in the built-in voices.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct NoteExpression {
    /// Release amount, 0 to 1, default 0.5. Multiplies envelope release
    /// duration by 1/8 to 8; its MIDI representation is release velocity.
    pub release: f32,
    /// Additional pitch in cents, -1200 to 1200, default 0.
    pub fine_pitch_cents: f32,
    /// Per-voice cutoff offset, 0 to 1, default 0.5 (no offset).
    pub modulation_x: f32,
    /// Per-voice resonance offset, 0 to 1, default 0.5 (no offset).
    pub modulation_y: f32,
    /// Slide bends held voices without retriggering; portamento starts a
    /// voice gliding from the most recent held pitch.
    #[ts(as = "Option<NoteArticulation>", optional)]
    pub articulation: NoteArticulation,
    /// Portamento duration in musical ticks. 1 to 245760, default 240.
    /// A slide uses the slide note's own length instead.
    #[ts(as = "Option<u32>", optional)]
    pub glide_ticks: u32,
    /// Optional note color group / zero-based MIDI channel, 0 through 15.
    /// None keeps the channel's display color and automatic MIDI routing.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub color_group: Option<u8>,
}

impl Default for NoteExpression {
    fn default() -> Self {
        Self::NEUTRAL
    }
}

impl NoteExpression {
    pub const NEUTRAL: Self = Self {
        release: 0.5,
        fine_pitch_cents: 0.0,
        modulation_x: 0.5,
        modulation_y: 0.5,
        articulation: NoteArticulation::Normal,
        glide_ticks: 240,
        color_group: None,
    };
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn valid(&self) -> bool {
        self.release.is_finite()
            && (0.0..=1.0).contains(&self.release)
            && self.fine_pitch_cents.is_finite()
            && (-1200.0..=1200.0).contains(&self.fine_pitch_cents)
            && self.modulation_x.is_finite()
            && (0.0..=1.0).contains(&self.modulation_x)
            && self.modulation_y.is_finite()
            && (0.0..=1.0).contains(&self.modulation_y)
            && (1..=245760).contains(&self.glide_ticks)
            && self.color_group.is_none_or(|group| group < 16)
    }

    pub fn clamped(self) -> Self {
        let value = |value: f32, min: f32, max: f32, default: f32| {
            if value.is_finite() {
                value.clamp(min, max)
            } else {
                default
            }
        };
        Self {
            release: value(self.release, 0.0, 1.0, 0.5),
            fine_pitch_cents: value(self.fine_pitch_cents, -1200.0, 1200.0, 0.0),
            modulation_x: value(self.modulation_x, 0.0, 1.0, 0.5),
            modulation_y: value(self.modulation_y, 0.0, 1.0, 0.5),
            articulation: self.articulation,
            glide_ticks: self.glide_ticks.clamp(1, 245760),
            color_group: self.color_group.map(|group| group.min(15)),
        }
    }

    pub fn release_multiplier(self) -> f32 {
        ((self.clamped().release - 0.5) * 6.0).exp2()
    }
    pub fn cutoff_octaves(self) -> f32 {
        (self.clamped().modulation_x - 0.5) * 8.0
    }
    pub fn resonance_multiplier(self) -> f32 {
        ((self.clamped().modulation_y - 0.5) * 4.0).exp2()
    }
}
