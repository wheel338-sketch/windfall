//! Additional stereo circuits. Construct/prepare off the audio thread; all
//! other Effect calls use fixed storage and a sample clock independent of blocks.
//! Registry integration and deliberately omitted overlapping circuits are
//! documented in docs/integration/seams/spatial.md.

mod band_delay;
mod chorus;
mod common;
mod flanger;
mod phaser;
mod room;
mod stereo;

pub use band_delay::{BandDelay, BandDelayParams};
pub use chorus::{HyperChorus, HyperChorusParams, VintageChorus, VintageChorusParams};
pub use flanger::{StackedFlanger, StackedFlangerParams};
pub use phaser::{VintagePhaser, VintagePhaserParams};
pub use room::{Room, RoomParams};
pub use stereo::{
    MAX_PHASE_OFFSET_MS, Spreader, SpreaderParams, StereoEnhancer, StereoEnhancerParams,
    StereoInvert,
};


#[cfg(test)]
mod tests;
