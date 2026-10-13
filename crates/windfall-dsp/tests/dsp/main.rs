//! Tests of the processors through the crate's public API. The shared
//! building blocks have their own tests next to their code.

mod bench;
mod compressor;
mod delay;
mod e3_circuits;
mod eq;
mod examples;
mod instrument_deadlines;
mod limiter;
mod limiter_dynamics;
mod modulation;
mod nonlinear_performance;
mod params;
mod properties;
mod realtime;
mod reverb;
mod slot;
mod support;
mod synth;
mod utilities;
mod utility_repairs;

/// Counts allocator calls for the realtime tests. It does nothing on a
/// thread that is not being watched, so the other tests are unaffected.
#[global_allocator]
static ALLOCATOR: realtime::CountingAllocator = realtime::CountingAllocator;

mod e1_cpu;
mod p2_effect_budgets;
mod p2_pitch_quality;
mod sample_bank;
mod sample_playback;

mod project_note_pan;
