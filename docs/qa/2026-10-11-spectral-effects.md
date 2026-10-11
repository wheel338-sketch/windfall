# Spectral effects implementation hand-off — 2026-10-11

Branch: `gpt/w3-spectral-effects`; base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.
This is an edit-only implementation. All tests below are **written, not executed**;
no build, test, benchmark, simulator regeneration or hardware check was run.
Parity status and generated artifacts are untouched.

## Per-row disposition

| Row | Claim | Implemented / remaining acceptance |
| --- | --- | --- |
| fx-fruity-convolver | Full-row candidate | Portable stereo IR import, immutable 128-frame FFT partitions, 65536-frame limit, legacy migration, checked history, original linear-phase EQ generator with persisted settings/group delay, dry alignment/PDC. Native publication, resource refusal and resampling quality still need acceptance. |
| fx-frequency-shifter | Full-row candidate | Additive Hz Hilbert shifter; independent sideband, cents-independent translation, measured 127-frame latency and runtime partition tests. Finite-FIR image rejection and out-of-band aliasing need listening. |
| fx-pitch-shifter | Full-row candidate | Ratio grains, independent formants, Mix/Voice modes, stereo source/filter processing and reported 1026-frame centre latency. LPC stability, quality and spectral measurements need execution. |
| fx-pitcher | Full-row candidate | Scale/root/tuning correction, shared YIN detector primitives, antiphase-safe tracking, real source-channel MIDI harmonies (four lowest distinct held keys), saved source and editor. Native short-note/live/export acceptance and quality remain. |
| fx-vocodex | Full-row candidate | Detailed 32-band bank, actual mixer key, role swap, explicit bandlimited synth carrier, silence without required source, bank-layout editor. Band-energy and native routing acceptance remain. |
| fx-fruity-vocoder | Full-row candidate | Classic 16-band mode of the same real routing/filter bank, distinct exposed layout. Same routing, stereo and envelope acceptance gates. |

“Full-row candidate” means source implementation plus authored acceptance, not a
verified parity claim. The overseer must not mark a row done before integration.

## Contracts / limits

IR assets live outside Copy scalar parameters. Plans retain immutable asset
snapshots and prepare resampling, FFT tables, kernels, histories and dry delay
storage before publication. Asset replacement forces a new owner; scalar changes
retain it. A 128 MiB preparation bound covers the new plan, not all retained plans
or process RSS. Oversized rate-expanded IRs refuse rather than become identity.
Offline attachment now uses the existing fallible admission and returns an empty
audio/error report on refusal instead of panicking through convenience attachment.
PCM is embedded in project JSON and individual effect/mixer-track presets. Legacy
inline IRs migrate before preset scalar cleanup. Preset loading compares the whole
asset for stale destination refusal and clears it when changing processor kinds.
Slow WAV reads are canceled after document revision/history changes. Generated EQ stores three gain controls and
symmetry-validated intentional delay; default convolution reports 128 frames,
generated 1025-tap EQ 640 at 48 kHz. WAV imports declare zero intentional delay.

Pitch Shift centre latency is 1026 frames; grain wet ages span 2–2050. Voice uses
order-16 LPC, Mix uses order eight for nonneutral formants. Pitch Correct tracks
60–1000 Hz through a decimated 256-frame direct-YIN window; shared first-trough
and parabolic helpers also serve the existing FFT analysis detector. At 48 kHz,
correction centre latency is 3074 frames. Four harmony voices are bounded to two
octaves and have 5 ms gain smoothing. Missing MIDI sources release voices. Engine
records admitted held keys per frame, so short notes do not become block snapshots.

Vocoders require real external carrier/modulator inputs unless the user explicitly
selects the synth carrier. There is no invented modulator. Classic has 16 bands,
detailed has 32; role swaps and actual layout are exposed. Layout is labeled as
filter layout, not a live energy meter. Synth is a bounded additive saw (up to 64
partials below 0.45 sample rate). Vocoders report zero transport latency.

## Written tests (not executed)

`crates/windfall-dsp/src/spectral/tests.rs`:
- `convolution_matches_a_direct_stereo_fir_beyond_legacy_capacity`,
  `convolver_is_partition_invariant`,
  `impulse_admission_refuses_empty_oversized_and_damaged_data`,
  `linear_phase_identity_reports_its_group_delay_and_aligns_the_dry_mix`.
- Existing identity latency plus
  `frequency_shift_translates_each_partial_by_hz_and_rejects_the_image`,
  `frequency_shifter_measured_identity_latency_is_127_frames`.
- `pitch_accuracy_is_measured_in_cents_and_formants_have_separate_controls`,
  `correction_detects_antiphase_and_midi_harmonies_release`; existing scale,
  grain latency, stereo, block partition and serialization coverage;
  `correction_measured_latency_tracks_the_input_sample_rate`.
- `vocoder_requires_a_nonzero_modulator_envelope_and_absent_key_is_silent`,
  `vocoder_bands_follow_the_real_modulator_and_preserve_carrier_balance`,
  `explicit_vocoder_synth_requires_modulator_and_key_carrier_routing_works`,
  `classic_and_detailed_vocoders_have_measured_zero_transport_delay`.

`crates/windfall-project/tests/spectral_effects.rs`:
`impulse_asset_history_clone_and_disk_roundtrip`,
`impulse_refusal_is_atomic_and_legacy_payload_migrates`,
`every_spectral_scalar_control_persists_and_undoes`,
`rate_conversion_preserves_stereo_and_refuses_runtime_overflow`,
`harmony_source_reference_persists_separately_from_scalar_automation`,
`linear_phase_delay_and_eq_controls_persist_and_invalid_metadata_is_refused`,
`convolver_plugin_presets_carry_assets_and_refuse_stale_destinations`,
`legacy_ir_and_harmony_source_survive_preset_migration`.

`crates/windfall-engine/tests/engine/spectral.rs` (registered in `main.rs`):
`spectral_live_processing_and_parameter_changes_never_allocate_or_free`,
`spectral_processors_are_live_partition_invariant`,
`classic_and_synth_vocoder_use_a_real_mixer_key`,
`midi_harmony_source_changes_audio_and_short_notes_are_partition_invariant`,
`replacing_a_prepared_ir_changes_the_live_owner_without_callback_destruction`,
`native_convolver_matches_direct_fir_and_offline_compensation`,
`linear_phase_eq_group_delay_is_in_public_mixer_compensation`,
`spectral_live_and_offline_audio_match_after_reported_compensation` (all six
behaviours at 44.1/48/96 kHz, including real key and short MIDI harmonies),
`refused_rate_expanded_ir_publication_preserves_the_sounding_owner`.

`apps/desktop/src/features/effects/spectral/impulse.test.ts`: strict WAV decoding,
malformed/oversized/nonfinite refusal, generated EQ symmetry and neutral identity.
`editors.test.tsx`: mounted IR import/clear/undo/error/EQ, saved scale/harmony
source, formant/mode controls, 16/32-band routing/synth layout, slow import
revision cancellation and same-name EQ control restoration on undo.
Simulator `EVERY_COMMAND` includes valid `setConvolverImpulse`.
Existing pitch-analysis tests must also run because YIN primitives were shared.

## Integrator commands (execute sequentially)

In Git Bash from the integration worktree, after merging:

```bash
source scripts/msvc-env.sh
bash scripts/gen-bindings.sh
bash scripts/build-sim.sh
export TS_RS_EXPORT_DIR="$(cygpath -w "$(mktemp -d)")"
cargo test -p windfall-core --lib
cargo test -p windfall-analysis --lib
cargo test -p windfall-dsp spectral
cargo test -p windfall-project --test spectral_effects
cargo test -p windfall-engine --test engine spectral
cargo test -p windfall-project --test presets
cargo test -p windfall-project -p windfall-ipc
cargo clippy -p windfall-core -p windfall-analysis -p windfall-dsp -p windfall-project -p windfall-engine --all-targets -- -D warnings
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop test src/features/effects/spectral src/lib/ipc/sim/document.test.ts
pnpm --dir apps/desktop lint
```

After software acceptance, perform native session save/reopen/undo and live/export
comparisons at 44.1/48/96 kHz, including failed prepared publication preserving
document/history, repeated large IR replacement/retirement, short MIDI harmonies,
key disconnect/reconnect, stereo/antiphase inputs and listening to voiced material.
Hardware/audio-device evidence stays external.

## Shared seams / regeneration

DSP `effect.rs`; project `model.rs`, `command.rs`, `lower.rs`, `check.rs`, `lib.rs`
preset state/apply/migration and affected slot literals; engine plan/state/rack/processor/render hosting and test
`main.rs`; core `lib.rs`; analysis pitch helper; frontend effect-editor registry
and simulator `EVERY_COMMAND`. See `git diff 87b87bc2 --name-only` for exact files.
No new dependencies or IPC endpoint; the existing document Command transports IRs.

Regenerate TypeScript `ConvolverParams`, `ConvolverImpulse`, `EffectSlot`,
`EffectParams`, `Command`, `PitchShiftParams`, `PitchShiftMode`,
`PitchCorrectParams`, `VocoderParams`, `PluginPresetState`, descriptor JSON, binding barrel and simulator
WASM/metadata. New descriptors append; existing automation indices are unchanged.

## Risks in check order

1. Hand-edited Rust hosting/result signatures, serde migration and widened params
   need compilation; generated TS shapes must precede frontend checking.
2. Prepared publication refusal and old-owner retirement need native transactional
   and callback allocation/free checks. The resource bound is per preparation.
3. LPC interpolation/warping quality and spectral-centroid thresholds are least
   certain. Test rather than weakening measurements; review vocal transients,
   formant identity and CPU at 96 kHz. Pitch grains are approximate resynthesis.
4. Per-frame MIDI owner capture, note ends and the fixed four-lowest selection
   need live/export and rapid note-event comparison. Source references may dangle
   intentionally and then supply silence.
5. Linear IR resampling has no anti-alias filter; resampled symmetric EQ delay is
   rounded. Quality at lower rates and impulse phase need listening.
6. Preset state now widens with optional IR metadata. Audit old/new effect and
   mixer-track presets, native load refusal and destination stale checks.
7. Frequency shifting outside the audible band and finite Hilbert rejection near
   DC/Nyquist are bounded-kernel limitations. No spectral or hardware result is claimed.
