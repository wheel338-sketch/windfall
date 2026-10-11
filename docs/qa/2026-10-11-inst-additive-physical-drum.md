# Additive, physical and drum instrument implementation â€” 2026-10-11

Branch: `worktree-agent-a10dd75da0e74d731`. Edit-only worker: no builds, tests,
benchmarks, generators or audio-device checks were run. Every test below is
**written, not executed**. Parity status is owned by the integrator.

## Row ledger

| Row | Implementation / claim | Remaining |
| --- | --- | --- |
| inst-boobass | Full-row candidate: Finger Bass now replaces its sole string on every positive note, including releases; 4 ms endpoint correction prevents retrigger discontinuities. Existing pluck, damping, pick position and two body resonances provide the bass-guitar tone contract. Six automatable controls retain descriptor indices. | Fresh integration execution and listening. |
| inst-harmor | Pending this stream. | Audio/image preparation and asset editor. |
| inst-morphine | Full-row candidate: new Snapshot Morph kind uses immutable worker-prepared 32-harmonic A/B spectra, phase/pan-aware interpolation, polyphony/envelope controls and a two-spectrum harmonic editor. | Fresh integration execution and listening. |
| inst-ogun | Pending this stream. | Metallic spectrum and editor acceptance. |
| inst-harmless | Pending this stream. | Subtractive harmonic controls and editor acceptance. |
| inst-autogun | Pending this stream. | Full number domain and preset-only workflow. |
| inst-beepmap | Pending this stream. | Color/brightness image scan. |
| inst-drumaxx | Full-row candidate: sixteen independent modal membrane/cavity patches, fixed MIDI 36–51, per-pad strike/damping/body controls, selection grid and hold-to-audition editor; shared passive model with single Membrane. | Fresh integration execution and listening. |
| inst-bassdrum | Full-row candidate: distinct Layer Kick kind, swept sine body plus optional immutable WAV layer, mix/tune/start controls; cancellable worker import, finite limits, remove/history and host asset identity. | Fresh integration execution and listening. |
| inst-drumpad | Full-row candidate: Membrane adds eight circular membrane eigenmodes plus a damped cavity, radial strike position, and frequency-dependent damping. One voice per channel, chromatic tuning, finite passive losses and deterministic noisy excitation; editor shows strike radius and grouped model controls. | Fresh integration execution and listening. |
| inst-fruity-drumsynth-live | Full-row candidate: sorted bank of up to 128 independent key patches; kick/snare/hat/tom synthesis, absolute key-program tuning, deterministic noise, bounded eight-voice hosting and a per-key editor with audition/history. | Fresh integration execution and listening. |

## Written acceptance

- `physical/tests.rs::finger_bass_is_monophonic_and_old_key_release_cannot_release_new_string`: one held/releasing string under note storms, old-key note-off isolation, bounded retrigger continuity and stop silence.
- `windfall-engine/tests/inst_additive_physical_drum.rs::finger_bass_tone_reaches_history_disk_and_live_offline_pcm`: audible tone change, undo/redo across render block sizes, actual disk reopen, public live/offline exact PCM.
- `inspector/instrument-section.test.tsx`: mounted Finger Bass tone edit/undo and all six controls.

All are **written, not executed**. Existing physical tests independently measure
tuning, decay, damping/body harmonic balance, pick position, extreme loop
stability, deterministic reset and partition invariance.

## Integrator commands

In Git Bash after `source scripts/msvc-env.sh`, redirect `TS_RS_EXPORT_DIR` to a
fresh temporary native path, then run:

```sh
cargo test -p windfall-dsp --lib physical
cargo test -p windfall-engine --test inst_additive_physical_drum
cargo test -p windfall-project --lib --tests
# Regenerate bindings/descriptors and simulator using the repository workflow.
pnpm --dir apps/desktop test src/features/channel-rack/inspector/instrument-section.test.tsx --maxWorkers=4
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```

## Integration notes and risks

- No new dependency, generated file edit, or parameter index change.
- The new engine test is an automatically discovered Cargo integration target.
- Inspect mono retrigger continuity and tone-difference threshold first; no runtime evidence is claimed.
- Shared seams touched so far: none.

### Single drum checkpoint

New `drums/modal.rs` owns the fixed modal response. `MembraneParams` appends
`strikePosition`, `damping`, `body` at indices 7–9; old indices 0–6 are preserved.
Missing fields receive defaults. All control edits retain automation/history.

Written, not executed: `drums/tests.rs::membrane_strike_damping_and_body_change_the_passive_modal_response`,
`membrane_has_measured_circular_mode_frequencies_and_transposes`, engine
`single_membrane_strike_reaches_history_disk_and_live_offline_pcm`, and mounted
single-membrane strike/undo test in `instrument-section.test.tsx`.
Run `cargo test -p windfall-dsp --lib drums` in addition to the commands above.
Regenerate `MembraneParams` and instrument descriptors/WASM. Shared seam:
`inspector/instrument-section.tsx` (editor routing only). Check the measured
modal-frequency and damping assertions first. The finite modal impulse model is
an original bounded approximation, not a vendor model/sound compatibility claim.

### Sixteen-pad checkpoint

`DrumPadParams` adds strike/damping/body with serde defaults. Drum Rack preserves
all 65 historical descriptors, appending three controls per pad at indices 65–112.
Written, not executed: DSP `rack_modal_model_edits_one_pad_without_changing_another`,
engine `sixteen_pad_membrane_model_reaches_history_disk_and_live_offline_pcm`,
mounted sixteenth-pad edit/undo test. Fresh artifacts must regenerate
`DrumPadParams`, `DrumRackParams` defaults/descriptors and simulator.
No sample-pad or zone-sampler claim. Shared editor routing seam is additive only.

### Per-key drum / immutable asset checkpoint

`instrument_asset.rs` was an existing unregistered grid/PCM foundation; its
schema and tests are retained, now exported, and extended with `DrumKeys`.
`ChannelSource::Instrument.asset` is optional and omitted when absent.
`SetInstrumentAsset` validates all data and compares the complete captured
source before editing; rejected worker results leave history/project untouched.
Ordinary parameter edits preserve assets. Channel cloning, duplication, presets,
save/reopen and existing channel patches carry the optional data automatically.
Engine preparation compares asset content on the control side, builds a fresh
instrument for changed assets, and retains immutable owners in plans/ledgers;
audio adopts prebuilt units and old plans retire through existing queues. Asset
edits replace the sounding program; they are not callback-side table mutations.

Written, not executed: `drum_key_bank_programs_are_absolute_tuned_independent_and_seeded`,
engine `per_key_drum_program_reaches_history_disk_and_live_offline_pcm`,
project `instrument_assets.rs` (history/stale/disk, invalid key programs, legacy
JSON), mounted arbitrary-key/undo case, and exhaustive simulator command example.
Run `cargo test -p windfall-project --test instrument_assets` and the updated
engine/drums/frontend commands. Regenerate `ChannelSource`, `Command`,
`InstrumentAsset`, `DrumKeyPatch`, `SpectralGrid`, `LayerPcm` and WASM.

Shared seams touched: DSP `instrument.rs`, `lib.rs`; project `model.rs`,
`command.rs`, `lower.rs`, `check.rs`, `automation.rs`, `preset.rs`; engine
`plan.rs`, `state.rs`, `rack.rs`; source constructor/destructure fixtures in
`engine/tests/engine/support.rs`, `engine/src/bin/windfall-soak.rs`,
`project/tests/commands.rs`, desktop `session/tests/effects.rs`; editor routing
and simulator `EVERY_COMMAND`. No new IPC method or patch section is needed.
Check source-variant constructors, `Command::Batch` integration and engine
asset-aware reuse first. Existing instrument kind order/count is unchanged here.

### Layer Kick checkpoint

Appends `LayerKick` after MidiOut, bringing this branch's registry to 28 kinds.
Every union/delegation arm is extended; the frontend exhaustive kind record and
Rust parameter contract are extended. Integrator must reconcile other workers'
append-only kinds and recompute ALL length. Core Kick remains separate.
PCM lives in `InstrumentAsset::Pcm`, never params. WAV decoding/downmixing,
quantization and long-source fade run in a dedicated browser worker; 16 MiB
input, 32768 output frames, 8–192 kHz, 1–8 channels, PCM16/24/32 or float32.
Jobs cancel on replacement/unmount and timeout at 30 seconds; attachment uses
complete expected-source guards. No proprietary content/dependency added.

Written, not executed: Layer Kick DSP sample-only independent PCM/partition
reference and missing-layer core identity; engine
`layer_kick_sample_changes_history_disk_and_live_offline_pcm`; Rust
`layer_kick_scalar_controls_round_trip_without_pcm_in_params`; worker WAV decode,
limit/fade/refusal tests; mounted layer remove/undo test.
Run `cargo test -p windfall-dsp --lib drums` and `cargo test -p windfall-dsp --test dsp`,
plus engine/project commands above. Include frontend
`src/features/channel-rack/instruments/asset-preparation.test.ts`,
`src/features/params/access.test.ts`, and `src/lib/ipc/sim/document.test.ts`.
Regenerate InstrumentKind/InstrumentParams, LayerKickParams, descriptors/WASM.
Check new enum arms and sample-only/missing-layer exact PCM first.

### Snapshot Morph checkpoint

Appends SnapshotMorph (29 kinds here). `SpectralBank`/`SpectralFrame`/
`SpectralPartial` are immutable heap assets, not Copy controls. Prepared playback
only reads them and updates oscillator/envelope state. Morph/filter gain arithmetic
is bounded playback arithmetic; no FFT, image decoding or spectral-table generation
runs on audio. All events/process/reset allocate/free nothing. Legacy PartialMorph,
Resynth, ScanSynth and HarmonicStack remain loadable foundation kinds; these new
full-row workflows do not use their old inline arrays.

Written, not executed: DSP `prepared_morph_is_exact_linear_interpolation_of_measured_pcm`,
`prepared_partial_amplitude_phase_and_pan_match_independent_sine_reference`, engine
`immutable_snapshot_morph_reaches_history_disk_and_live_offline_pcm`, snapshot
worker mapping/refusal case, mounted two-spectrum/morph/undo case.
Run `cargo test -p windfall-dsp --lib additive` with existing family commands.
Regenerate SnapshotMorphParams plus the three spectral asset types, union and
descriptors/WASM. Check phase/pan/reference and per-sample partition arithmetic.
