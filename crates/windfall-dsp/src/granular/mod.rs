//! Prepared-source slice, granular and drawn-position/speed instruments.
mod asset;
mod engine;
pub use asset::{
    MAX_ASSET_SLICES, MAX_CURVE_POINTS, MAX_SOURCE_FRAMES, PlaybackAsset, PlaybackCurve,
    PlaybackCurveMode, PlaybackPoint, SliceRegion, validate_slices,
};
use std::sync::Arc;
mod params;

use crate::instrument::Instrument;
use crate::param::ParamSet;
use crate::{NoteExpression, NoteInstanceId};
use engine::{Engine, Mode, Settings};
pub use params::{
    GrainCloudParams, SampleTable, SliceDeckParams, SliceMapParams, SliceSettings, WaveRideParams,
};

pub const MAX_SAMPLE_FRAMES: usize = 4096;
pub const SLICE_COUNT: usize = 16;
pub const MAX_POLYPHONY: usize = 16;
pub const MAX_GRAINS_PER_VOICE: usize = 32;

macro_rules! instrument {
    ($name:ident, $params:ident, $mode:ident) => {
        /// Fixed-storage realtime instrument; construction and prepare belong off audio.
        pub struct $name {
            engine: Engine,
            params: $params,
            current: $params,
            moving: bool,
            asset: Option<Arc<PlaybackAsset>>,
        }
        impl Default for $name {
            fn default() -> Self {
                Self {
                    engine: Engine::new(Mode::$mode),
                    params: $params::default(),
                    current: $params::default(),
                    moving: false,
                    asset: None,
                }
            }
        }
        impl $name {
            /// Control-thread only: install before publishing the prepared unit.
            pub fn set_asset(&mut self, asset: Arc<PlaybackAsset>) {
                self.asset = Some(asset);
            }
            pub fn voices_stolen(&self) -> u64 {
                self.engine.voices_stolen
            }
            pub(crate) fn adopt(&mut self, previous: &Self) {
                self.engine.adopt(&previous.engine);
                self.current = previous.current;
                self.moving = true;
            }
        }
        impl Instrument for $name {
            fn set_note_pan_law(&mut self, law: windfall_core::PanLaw) {
                self.engine.set_note_pan_law(law);
            }
            type Params = $params;
            fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
                self.engine.prepare(sample_rate);
                self.reset();
            }
            fn reset(&mut self) {
                self.engine.reset();
                self.current = self.params;
                self.moving = false;
            }
            fn set_params(&mut self, params: &$params) {
                self.params = params.sanitized().with_legacy_changes(&self.params);

                if self.engine.fresh {
                    self.current = self.params;
                    self.moving = false;
                } else {
                    self.moving = true;
                }
            }
            fn note_on(&mut self, key: u8, velocity: f32) {
                self.note_on_expression(key, velocity, 0.0, NoteExpression::default());
            }
            fn note_on_expression(
                &mut self,
                key: u8,
                velocity: f32,
                pan: f32,
                expression: NoteExpression,
            ) {
                if !velocity.is_finite() || velocity <= 0.0 {
                    self.note_off(key);
                    return;
                }
                let id = self.engine.legacy_id();
                self.note_on_instance(id, key, velocity, pan, expression);
            }
            fn supports_note_instances(&self) -> bool {
                true
            }
            fn note_on_instance(
                &mut self,
                id: NoteInstanceId,
                key: u8,
                velocity: f32,
                pan: f32,
                expression: NoteExpression,
            ) {
                if !velocity.is_finite() || velocity <= 0.0 {
                    self.note_off_instance(id, key);
                    return;
                }
                self.engine.note_on(
                    id,
                    key,
                    velocity,
                    pan,
                    expression,
                    self.current.settings(),
                    self.asset.as_deref(),
                );
            }
            fn note_off(&mut self, key: u8) {
                self.engine
                    .release_key(key.min(127), self.current.release_ms);
            }
            fn note_off_instance(&mut self, id: NoteInstanceId, _key: u8) {
                self.engine.release_instance(id, self.current.release_ms);
            }
            fn set_note_expression(
                &mut self,
                id: NoteInstanceId,
                pan: f32,
                expression: NoteExpression,
            ) {
                self.engine.expression(id, pan, expression);
            }
            fn set_note_pitch(&mut self, id: NoteInstanceId, pitch: f32) {
                self.engine.pitch(id, pitch);
            }
            fn set_note_gain(&mut self, id: NoteInstanceId, gain: f32) {
                self.engine.gain(id, gain);
            }
            fn all_notes_off(&mut self) {
                self.engine.stop();
            }
            fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
                left.fill(0.0);
                right.fill(0.0);
                for (left, right) in left.iter_mut().zip(right.iter_mut()) {
                    if self.engine.control_due() && self.moving {
                        self.moving = self.current.approach(&self.params, self.engine.smoothing);
                    }
                    let (l, r) = self
                        .engine
                        .tick(self.current.settings(), self.asset.as_deref());
                    *left = l;
                    *right = r;
                }
            }
            fn active_voices(&self) -> usize {
                self.engine.active_voices()
            }
        }
    };
}
instrument!(SliceMap, SliceMapParams, Map);
instrument!(SliceDeck, SliceDeckParams, Deck);
instrument!(GrainCloud, GrainCloudParams, Cloud);
instrument!(WaveRide, WaveRideParams, Wave);

impl GrainCloud {
    pub fn grains_dropped(&self) -> u64 {
        self.engine.grains_dropped
    }
    /// Successfully scheduled grain events since prepare/reset, across all voices.
    /// Excess events at the fixed overlap limit drop without being deferred.
    pub fn grains_spawned(&self) -> u64 {
        self.engine.grains_spawned
    }
}

impl SliceMapParams {
    fn settings(&self) -> Settings {
        Settings {
            key: self.base_key,
            level: self.level,
            legacy_starts: self.slice_starts,
            legacy_edits: self.legacy_edits,
            ..Settings::default()
        }
    }
}
impl SliceDeckParams {
    fn settings(&self) -> Settings {
        Settings {
            key: self.base_key,
            level: self.level,
            legacy_starts: self.slice_starts,
            legacy_slices: self.slices,
            legacy_edits: self.legacy_edits,
            ..Settings::default()
        }
    }
}
impl GrainCloudParams {
    fn settings(&self) -> Settings {
        Settings {
            key: self.root_key,
            grain_ms: self.grain_size_ms,
            density: self.density,
            position: self.position,
            spray: self.spray,
            pitch: self.pitch,
            seed: self.seed,
            level: self.level,
            ..Settings::default()
        }
    }
}
impl WaveRideParams {
    fn settings(&self) -> Settings {
        Settings {
            key: self.root_key,
            duration_ms: self.duration_ms,
            pitch: self.pitch,
            level: self.level,
            legacy_positions: self.positions,
            legacy_edits: self.legacy_edits,
            ..Settings::default()
        }
    }
}

#[cfg(test)]
mod tests;
