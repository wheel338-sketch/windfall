//! Drawn curves, asymmetric overdrive, guitar pedals and a series drive chain.
//!
//! The nonlinear stages run above the host rate through `oversample`, which
//! delays by [`DRIVE_LATENCY_SAMPLES`] at every quality setting. See
//! `docs/integration/seams/drive.md` for what each processor does.

mod cabinet;
mod chain;
mod guitar;
mod overdrive;
mod oversample;
mod waveshaper;

pub use chain::{CHAIN_LATENCY_SAMPLES, DriveChain, DriveChainParams, DriveStage, DriveStageParams};
pub use cabinet::{Cabinet, MAX_CABINET_TAPS, cabinet_response, cabinet_taps};
pub use guitar::{GuitarModel, GuitarRack, GuitarRackParams, PedalParams};
pub use overdrive::{Overdrive, OverdriveParams};
pub use oversample::{DRIVE_LATENCY_SAMPLES, Oversampling};
pub use waveshaper::{CURVE_POINTS, IDENTITY_CURVE, Waveshaper, WaveshaperParams};

/// Different positive and negative ceilings, continuous with unit slope at
/// zero. Unlike a biased clipper, silence still maps exactly to silence.
#[inline]
fn asymmetric(input: f32, asymmetry: f32) -> f32 {
    let ceiling = if input >= 0.0 {
        1.0 - 0.45 * asymmetry
    } else {
        1.0 + 0.25 * asymmetry
    };
    input / (1.0 + input.abs() / ceiling)
}

#[cfg(test)]
mod shaping_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod guitar_rack_tests;
