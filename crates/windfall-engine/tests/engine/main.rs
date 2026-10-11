//! Tests of the engine through its public API. None of them needs an audio
//! device: they drive a `Processor` directly or use the offline renderer.

mod audio_clips;
mod audio_logger;
mod automation;
mod device;
mod dynamics_mastering;
mod effects;
mod expression_map;
mod filter_family;
mod instruments;
mod lofi;
mod mixer;
mod modulation;
mod nonlinear_performance;
mod playlist_layouts;
mod playlist_stems;
mod project_settings;
mod rack_voice;
mod realtime;
mod rendering;
mod sampler;
mod sampler_processing;
mod sequencing;
mod stems;
mod support;
mod timeline;
mod utility_effects;

/// Counts allocator calls for the realtime tests. It does nothing on a
/// thread that is not being watched, so the other tests are unaffected.
#[global_allocator]
static ALLOCATOR: realtime::CountingAllocator = realtime::CountingAllocator;

mod sample_playback;
mod spectral;
