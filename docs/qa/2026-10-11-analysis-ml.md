# Audio analysis / ML implementation hand-off — 2026-10-11

Branch: `gpt/w3-analysis-ml`, base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.
Worker follows the common brief: edits only, no builds/tests, no agents, no
downloads, no external dependencies, no generated bindings/WASM edits.

## Implementation in progress

Classical kernels: `crates/windfall-analysis/src/classical.rs`.
Persistent estimates, overrides, warp anchors and noise-profile controls:
`crates/windfall-project/src/audio_analysis.rs`, optional default-omitted
`SampleAsset.processing`, checked `SetSampleProcessing` command. Original
samples referenced by recipes cannot be removed. Existing project and stretch
workspace crates are reused by analysis; Cargo.lock dependency edges reflect
those additions. No new external crate or model weights.

Written, not executed: `crates/windfall-analysis/tests/classical_tools.rs`:
`calibrated_note_pitch_duration_and_neutral_render`,
`repeated_and_held_notes_silence_and_polyphonic_chord`,
`glide_vibrato_and_noise_keep_bounded_estimates`,
`warp_order_bounds_overrides_and_cancel_are_explicit`,
`learned_profile_improves_independent_snr_and_dry_is_exact`.

These assertions are acceptance targets, not measurements. The SNR fixture
requires >5 dB improvement and <1.5 dB tone RMS error; no result is claimed.
The existing calibrated pitch suite is retained unchanged.

## Remaining integration work

Native editor review/render/audition and one-step import; purpose-built frontend;
model-backed stem/remix/dereverb selection and import contracts; model/runtime
licence evaluation. All seven rows remain partial until the final scope and
integrator verification below are recorded.

## Integrator commands (worker has not run any of them)

Use Git Bash with `source scripts/msvc-env.sh`, one Cargo owner, and a temporary
native `TS_RS_EXPORT_DIR`.

```sh
cargo test -p windfall-analysis --locked --offline --test classical_tools
cargo test -p windfall-analysis --locked --offline --test pitch_analysis
cargo test -p windfall-project --locked --offline
cargo clippy -p windfall-analysis -p windfall-project --all-targets --locked --offline -- -D warnings
```

Check first: type export of the recipe/Clip references, new sample literals,
neutral render identity and independent SNR/frequency assertions. No listening,
hardware, cross-platform or model-quality evidence exists for this change.
