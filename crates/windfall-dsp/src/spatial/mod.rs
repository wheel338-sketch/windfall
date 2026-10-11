//! Additional stereo circuits. Construct/prepare off the audio thread; all
//! other Effect calls use fixed storage and a sample clock independent of blocks.
//! Registry integration and deliberately omitted overlapping circuits are
//! documented in docs/integration/seams/spatial.md.

mod band_delay;
mod bucket;
mod chorus;
mod common;
mod flanger;
mod phaser;
mod room;
mod stereo;

pub use band_delay::{BandDelay, BandDelayParams};
pub use bucket::{BUCKET_GUARD_HZ, BUCKET_SAMPLES, BucketMode, BucketSweep};
pub use chorus::{
    HyperChorus, HyperChorusParams, SWARM_MAX_VOICES, VintageChorus, VintageChorusParams,
    voice_delay_ratio, voice_rate_ratio,
};
pub use flanger::{FLANGER_MAX_ORDER, StackedFlanger, StackedFlangerParams, comb_time_ratio};
pub use phaser::{
    PHASER_MAX_STAGES, PhaserStages, VintagePhaser, VintagePhaserParams, stage_ratio,
};
pub use room::{Room, RoomParams};
pub use stereo::{
    MAX_PHASE_OFFSET_MS, Spreader, SpreaderParams, StereoEnhancer, StereoEnhancerParams,
    StereoInvert,
};

#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod tests;
