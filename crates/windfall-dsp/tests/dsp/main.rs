//! Tests of the processors through the crate's public API. The shared
//! building blocks have their own tests next to their code.

mod bench;
mod compressor;
mod delay;
mod eq;
mod examples;
mod limiter;
mod limiter_dynamics;
mod modulation;
mod params;
mod properties;
mod realtime;
mod reverb;
mod slot;
mod support;
mod synth;
mod utilities;
mod utility_repairs;
mod e3_circuits;
mod nonlinear_performance;

/// Counts allocator calls for the realtime tests. It does nothing on a
/// thread that is not being watched, so the other tests are unaffected.
#[global_allocator]
static ALLOCATOR: realtime::CountingAllocator = realtime::CountingAllocator;

mod sample_playback;
