//! Windfall's fixed-band EQ, snapshot morph EQ, and eight-slot audio filter bank.
//! All state is inline, controls glide over 5 ms, and no processor adds latency.

mod filter_bank;
mod graphic;
mod morph;
mod pattern;
mod seven;
mod stage;

pub use filter_bank::{
    FilterBank, FilterBankParams, FilterRouting, FilterSlotMode, FilterSlotParams,
};
pub use graphic::{
    MORPH_CURVE_BANDS, MORPH_CURVE_FREQUENCIES_HZ, MORPH_CURVE_Q, MORPH_CURVES, MorphCurveParams,
};
pub use morph::{MorphBandParams, MorphEq, MorphEqParams, MorphSnapshotParams};
pub use pattern::{FILTER_PATTERN_POINTS, FilterPatternParams};
pub use seven::{SEVEN_BAND_FREQUENCIES_HZ, SevenBand, SevenBandParams};

#[cfg(test)]
mod tests;
