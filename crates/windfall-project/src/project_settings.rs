//! Project-wide playback choices beside tempo and meter: the panning law.
//!
//! The timebase, the other such choice, is in [`crate::timebase`].

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How panning changes level, for the whole project.
///
/// It governs every pan the engine applies itself: a channel's pan, a
/// sampler note's pan on top of it, an audio clip's pan and a mixer track's
/// pan, in playback and in an export alike. Every law is unity at the
/// center, so a project in which nothing is panned sounds the same under all
/// of them. [`windfall_core::PanLaw`] has the formulas.
///
/// Built-in instance-aware voices also use this law. Native note expressions
/// carry its stereo balance and level where the plugin supports them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PanLaw {
    /// The near side stays at 0 dB and the far side fades in a straight
    /// line. What projects saved before the setting existed use.
    #[default]
    Balance,
    /// Equal power at every position; the near side reaches +3 dB.
    ConstantPower,
    /// Between constant power and linear; the near side reaches +4.5 dB.
    Compromise,
    /// The two sides always sum to the same level; the near side reaches
    /// +6 dB.
    Linear,
}

impl PanLaw {
    pub const ALL: [PanLaw; 4] = [
        PanLaw::Balance,
        PanLaw::ConstantPower,
        PanLaw::Compromise,
        PanLaw::Linear,
    ];

    /// Whether this is the law a file that names none has.
    pub fn is_default(&self) -> bool {
        *self == PanLaw::default()
    }

    /// The same law as the engine's own type.
    pub fn core(self) -> windfall_core::PanLaw {
        match self {
            PanLaw::Balance => windfall_core::PanLaw::Balance,
            PanLaw::ConstantPower => windfall_core::PanLaw::ConstantPower,
            PanLaw::Compromise => windfall_core::PanLaw::Compromise,
            PanLaw::Linear => windfall_core::PanLaw::Linear,
        }
    }

    /// Left and right gain at a pan position from -1 to 1.
    pub fn gains(self, pan: f32) -> (f32, f32) {
        self.core().gains(pan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_law_is_stored_by_name_and_omitted_laws_are_the_legacy_balance() {
        assert_eq!(
            serde_json::to_string(&PanLaw::ConstantPower).unwrap(),
            "\"constantPower\""
        );
        for law in PanLaw::ALL {
            let json = serde_json::to_string(&law).unwrap();
            assert_eq!(serde_json::from_str::<PanLaw>(&json).unwrap(), law);
            assert_eq!(law.gains(0.25), law.core().gains(0.25));
        }
        assert!(PanLaw::Balance.is_default());
        assert!(!PanLaw::Linear.is_default());
        assert!(serde_json::from_str::<PanLaw>("\"circular\"").is_err());
    }
}
