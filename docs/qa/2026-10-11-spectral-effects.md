# Spectral effects implementation hand-off — 2026-10-11

Branch: `gpt/w3-spectral-effects`. All checks below are **written, not executed**.
This worker only reads/edits/commits; the integrator owns builds and execution.

## Current work

- Convolver: portable stereo PCM asset outside Copy parameters; legacy mono IR
  migration; checked undoable command; immutable control-side partition preparation;
  rate conversion with visible runtime-limit refusal; aggregate preparation admission;
  WAV loader, clear and own symmetric three-region linear-phase EQ generator/editor.
  Source/runtime IR limit is 65536 frames. Transport latency is 128 frames; a
  symmetric 1025-tap EQ adds 512 frames intentional filter group delay at 48 kHz.
  Full-row candidate still depends on native history/publication/audio acceptance.
- Frequency Shifter: existing additive Hz kernel retained; strengthened measurements
  and runtime acceptance are in progress.
- Classic / detailed Vocoder: real key input required; absent key is silence;
  key/carrier role selection and explicit bandlimited additive carrier synth added;
  dedicated band-layout/routing editor. Envelope display is a filter layout, not
  a fabricated live meter. Acceptance tests are in progress.
- Pitch Shift / Pitch Correct: existing limitations still apply while follow-up
  implementation proceeds; no claim of independent formants or MIDI harmonies yet.

## Written tests (not executed)

`crates/windfall-dsp/src/spectral/tests.rs`: identity measured latency;
`convolution_matches_a_direct_stereo_fir_beyond_legacy_capacity`;
`convolver_is_partition_invariant`;
`impulse_admission_refuses_empty_oversized_and_damaged_data`;
absent-key vocoder behaviour. Existing pitch/frequency tests remain registered.

`crates/windfall-project/tests/spectral_effects.rs`:
`impulse_asset_history_clone_and_disk_roundtrip`;
`impulse_refusal_is_atomic_and_legacy_payload_migrates`;
`every_spectral_scalar_control_persists_and_undoes`;
`rate_conversion_preserves_stereo_and_refuses_runtime_overflow`.

Simulator EVERY_COMMAND gains a valid `setConvolverImpulse` example.

## Integrator commands

Run sequentially in Git Bash after `source scripts/msvc-env.sh`; set a private
`TS_RS_EXPORT_DIR` before tests. Regenerate bindings/descriptors/WASM first.

```bash
cargo test -p windfall-dsp spectral
cargo test -p windfall-project --test spectral_effects
cargo test -p windfall-engine --test engine spectral
cargo clippy -p windfall-dsp -p windfall-project -p windfall-engine --all-targets -- -D warnings
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```

## Risks / remaining gates

1. Hand-edited shared hosting signatures and asset-generation reuse need compilation.
2. Native prepared publication refusal must preserve document/history when resampling
   exceeds runtime admission; no identity fallback is allowed.
3. Asset replacement continuity uses existing replacement/splice hosting; audio
   allocator and live/export checks must prove old assets retire off the callback.
4. Rate conversion is linear interpolation, without an anti-alias resampler; high
   frequency IR content at lower destination rates needs listening/quality review.
5. FIR group delay is additional to reported convolution transport latency; future
   PDC integration may require persisted intentional-delay metadata.
6. Pitch/formant and MIDI harmony completion is still outstanding.

Generated types to refresh: ConvolverParams, EffectParams, EffectSlot,
ConvolverImpulse, VocoderParams, Command, descriptors and simulator WASM.
