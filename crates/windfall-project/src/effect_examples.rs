//! Original, asset-free examples in the existing Windfall plugin-preset format.
//! These are control-side content; none of this module is used by audio callbacks.

use crate::{EffectKind, Preset};

/// The parameter examples and their descriptive names are dedicated to CC0 1.0.
pub const EFFECT_EXAMPLE_LICENSE: &str = "CC0-1.0";

/// A loadable example, with its file shipped in docs/examples/dynamics-eq.
pub struct EffectExample {
    pub kind: EffectKind,
    pub name: &'static str,
    pub file_name: &'static str,
    pub bytes: &'static [u8],
    /// A self-contained original phrase for File -> Open, with the same patch.
    pub project_json: &'static str,
}

impl EffectExample {
    /// Uses precisely the reader used for user-saved plugin presets. Omitted
    /// settings receive the reader's legacy defaults, with its explanatory notes.
    pub fn load(&self) -> Result<(Preset, Vec<String>), String> {
        Preset::from_json(self.bytes)
    }
}

/// Append examples here; existing effect/parameter registries are unchanged.
pub const EFFECT_EXAMPLES: &[EffectExample] = &[
    EffectExample {
        kind: EffectKind::TransientShaper,
        name: "Attack lift",
        file_name: "attack-lift.wfpreset",
        project_json: include_str!("../../../docs/examples/dynamics-eq/attack-lift.windfall"),
        bytes: include_bytes!("../../../docs/examples/dynamics-eq/attack-lift.wfpreset"),
    },
    EffectExample {
        kind: EffectKind::MasterLimiter,
        name: "Gentle level and color",
        file_name: "gentle-level-and-color.wfpreset",
        project_json: include_str!(
            "../../../docs/examples/dynamics-eq/gentle-level-and-color.windfall"
        ),
        bytes: include_bytes!("../../../docs/examples/dynamics-eq/gentle-level-and-color.wfpreset"),
    },
    EffectExample {
        kind: EffectKind::BassHarmonics,
        name: "Bass octave support",
        file_name: "bass-octave-support.wfpreset",
        project_json: include_str!(
            "../../../docs/examples/dynamics-eq/bass-octave-support.windfall"
        ),
        bytes: include_bytes!("../../../docs/examples/dynamics-eq/bass-octave-support.wfpreset"),
    },
    EffectExample {
        kind: EffectKind::MultibandMaximizer,
        name: "Balanced band lift",
        file_name: "balanced-band-lift.wfpreset",
        project_json: include_str!(
            "../../../docs/examples/dynamics-eq/balanced-band-lift.windfall"
        ),
        bytes: include_bytes!("../../../docs/examples/dynamics-eq/balanced-band-lift.wfpreset"),
    },
    EffectExample {
        kind: EffectKind::FrequencyDelay,
        name: "Alternating short band echoes",
        file_name: "alternating-short-band-echoes.wfpreset",
        project_json: include_str!(
            "../../../docs/examples/dynamics-eq/alternating-short-band-echoes.windfall"
        ),
        bytes: include_bytes!(
            "../../../docs/examples/dynamics-eq/alternating-short-band-echoes.wfpreset"
        ),
    },
    EffectExample {
        kind: EffectKind::Spreader,
        name: "Short stereo space",
        file_name: "short-stereo-space.wfpreset",
        project_json: include_str!(
            "../../../docs/examples/dynamics-eq/short-stereo-space.windfall"
        ),
        bytes: include_bytes!("../../../docs/examples/dynamics-eq/short-stereo-space.wfpreset"),
    },
    EffectExample {
        kind: EffectKind::MorphEq,
        name: "Low to high curve blend",
        file_name: "low-to-high-curve-blend.wfpreset",
        project_json: include_str!(
            "../../../docs/examples/dynamics-eq/low-to-high-curve-blend.windfall"
        ),
        bytes: include_bytes!(
            "../../../docs/examples/dynamics-eq/low-to-high-curve-blend.wfpreset"
        ),
    },
    EffectExample {
        kind: EffectKind::BandSplit,
        name: "Three band tilt",
        file_name: "three-band-tilt.wfpreset",
        project_json: include_str!("../../../docs/examples/dynamics-eq/three-band-tilt.windfall"),
        bytes: include_bytes!("../../../docs/examples/dynamics-eq/three-band-tilt.wfpreset"),
    },
    EffectExample {
        kind: EffectKind::EchoBank,
        name: "Two linked filtered echoes",
        file_name: "two-linked-filtered-echoes.wfpreset",
        project_json: include_str!(
            "../../../docs/examples/dynamics-eq/two-linked-filtered-echoes.windfall"
        ),
        bytes: include_bytes!(
            "../../../docs/examples/dynamics-eq/two-linked-filtered-echoes.wfpreset"
        ),
    },
    EffectExample {
        kind: EffectKind::SelectableFilter,
        name: "Soft resonant lowpass",
        file_name: "soft-resonant-lowpass.wfpreset",
        project_json: include_str!(
            "../../../docs/examples/dynamics-eq/soft-resonant-lowpass.windfall"
        ),
        bytes: include_bytes!("../../../docs/examples/dynamics-eq/soft-resonant-lowpass.wfpreset"),
    },
    EffectExample {
        kind: EffectKind::FilterBank,
        name: "Alternating drawn gate",
        file_name: "alternating-drawn-gate.wfpreset",
        project_json: include_str!(
            "../../../docs/examples/dynamics-eq/alternating-drawn-gate.windfall"
        ),
        bytes: include_bytes!("../../../docs/examples/dynamics-eq/alternating-drawn-gate.wfpreset"),
    },
    EffectExample {
        kind: EffectKind::MultibandCompressor,
        name: "Gentle three band control",
        file_name: "gentle-three-band-control.wfpreset",
        project_json: include_str!(
            "../../../docs/examples/dynamics-eq/gentle-three-band-control.windfall"
        ),
        bytes: include_bytes!(
            "../../../docs/examples/dynamics-eq/gentle-three-band-control.wfpreset"
        ),
    },
    EffectExample {
        kind: EffectKind::PanLfo,
        name: "Slow pan and pulse",
        file_name: "slow-pan-and-pulse.wfpreset",
        project_json: include_str!(
            "../../../docs/examples/dynamics-eq/slow-pan-and-pulse.windfall"
        ),
        bytes: include_bytes!("../../../docs/examples/dynamics-eq/slow-pan-and-pulse.wfpreset"),
    },
    EffectExample {
        kind: EffectKind::StereoEnhancer,
        name: "Right offset and narrow sides",
        file_name: "right-offset-and-narrow-sides.wfpreset",
        project_json: include_str!(
            "../../../docs/examples/dynamics-eq/right-offset-and-narrow-sides.windfall"
        ),
        bytes: include_bytes!(
            "../../../docs/examples/dynamics-eq/right-offset-and-narrow-sides.wfpreset"
        ),
    },
    EffectExample {
        kind: EffectKind::OneKnob,
        name: "Moderate band enhancement",
        file_name: "moderate-band-enhancement.wfpreset",
        project_json: include_str!(
            "../../../docs/examples/dynamics-eq/moderate-band-enhancement.windfall"
        ),
        bytes: include_bytes!(
            "../../../docs/examples/dynamics-eq/moderate-band-enhancement.wfpreset"
        ),
    },
    EffectExample {
        kind: EffectKind::StageStack,
        name: "Warm safety stages",
        file_name: "warm-safety-stages.wfpreset",
        project_json: include_str!(
            "../../../docs/examples/dynamics-eq/warm-safety-stages.windfall"
        ),
        bytes: include_bytes!("../../../docs/examples/dynamics-eq/warm-safety-stages.wfpreset"),
    },
    EffectExample {
        kind: EffectKind::SevenBand,
        name: "Low mid cleanup",
        file_name: "low-mid-cleanup.wfpreset",
        project_json: include_str!("../../../docs/examples/dynamics-eq/low-mid-cleanup.windfall"),
        bytes: include_bytes!("../../../docs/examples/dynamics-eq/low-mid-cleanup.wfpreset"),
    },
];
