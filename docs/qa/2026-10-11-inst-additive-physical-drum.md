# Additive, physical and drum instrument implementation â€” 2026-10-11

Branch: `worktree-agent-a10dd75da0e74d731`. Edit-only worker: no builds, tests,
benchmarks, generators or audio-device checks were run. Every test below is
**written, not executed**. Parity status is owned by the integrator.

## Row ledger

| Row | Implementation / claim | Remaining |
| --- | --- | --- |
| inst-boobass | Full-row candidate: Finger Bass now replaces its sole string on every positive note, including releases; 4 ms endpoint correction prevents retrigger discontinuities. Existing pluck, damping, pick position and two body resonances provide the bass-guitar tone contract. Six automatable controls retain descriptor indices. | Fresh integration execution and listening. |
| inst-harmor | Full-row candidate: Spectral Resynth imports harmonic WAV analysis or luminance image spectra on a worker, stores amplitude/phase frames, provides manual position or timed scan and a spectral preview. | Fresh integration execution and listening. |
| inst-morphine | Full-row candidate: new Snapshot Morph kind uses immutable worker-prepared 32-harmonic A/B spectra, phase/pan-aware interpolation, polyphony/envelope controls and a two-spectrum harmonic editor. | Fresh integration execution and listening. |
| inst-ogun | Full-row candidate: Metal Voice has prepared inharmonic partials, partial stretch, independent phase-offset amplitude shimmer and exponential decay; dedicated spacing/shimmer editor. | Fresh integration execution and listening. |
| inst-harmless | Full-row candidate: Harmonic Filter plays an immutable 64-harmonic bank through saw/square/triangle weighting, spectral cutoff/resonance and envelope/polyphony controls; dedicated oscillator/filter/envelope editor. | Fresh integration execution and listening. |
| inst-autogun | Full-row candidate: Number Voice derives immutable 64-partial patches deterministically from any unsigned 32-bit number; preset-only editor and output level. | Fresh integration execution and listening. |
| inst-beepmap | Full-row candidate: Color Scan maps luminance/alpha to amplitude, rows/hue to semitone frequencies, red/blue to stereo; timed scan/manual freeze, speed/width editor and immutable spectral preview. | Fresh integration execution and listening. |
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

### Harmonic Filter checkpoint

Appends HarmonicFilter (30 kinds here); waveform/cutoff/resonance are scalar
controls applied to immutable base harmonics. No callback-created partial bank.
Written, not executed: DSP `subtractive_controls_shape_an_additive_harmonic_spectrum`
measures even-harmonic rejection, square/triangle harmonic ratios, low-pass loss
and resonant gain; engine `harmonic_filter_cutoff_reaches_history_disk_and_live_offline_pcm`;
mounted response/cutoff/history case. Regenerate HarmonicShape/HarmonicFilterParams
and union/descriptors/WASM. Run additive, engine and mounted suites above.

### Metal Voice checkpoint

Appends MetalVoice (31 kinds here). Written, not executed: DSP
`metallic_partials_shimmer_decay_and_reset_are_measured` (off-harmonic peaks,
audible shimmer, decay silence, reproducibility), engine
`metallic_shimmer_reaches_history_disk_and_live_offline_pcm`, mounted shimmer/display/history.
Regenerate MetalVoiceParams and union/descriptors/WASM. The base bank is made
at construction/preparation, held immutable, and read by the same bounded engine.

### Number Voice checkpoint

Appends NumberVoice (32 kinds here). Selection stays as u32 asset metadata,
never a float descriptor, so all 4294967296 patch numbers remain distinguishable.
Worker and Rust preparation use xorshift32, f64 arithmetic then f32 storage;
persisted partials are authoritative across platforms (pow rounding can differ
by an ulp). Written, not executed: numbered DSP reproducibility/audio differences,
engine full-width asset history/disk/live equality, worker boundary/refusal and
mounted preset-only workflow. Regenerate NumberVoiceParams and union/descriptors.

### Spectral Resynth checkpoint

Appends SpectralResynth (33 kinds here). Dedicated worker performs bounded
harmonic projection of PCM at an explicit source root; it also maps image
luminance/alpha, bottom row to fundamental, columns to time. Source decoding
uses browser image decoding off audio; worker owns all spectral preparation.
No proprietary analysis or vendor preset compatibility claim. Analysis retains
32768 mono samples, at most 256 frames and 64 partials; harmonic projection
omits DC and unpitched/noisy residuals. Root mismatch/nonperiodic sources have
higher reconstruction error; the acceptance bound applies to coherent in-band
harmonic fixtures, not arbitrary recordings. Source files cap at 16 MiB.
Written, not executed: worker independent three-harmonic reconstruction error
and image row/alpha/luminance mapping; DSP static phase reconstruction and scan
differences; engine frame edit history/disk/live equality; mounted source/frame
workflow. Phase interpolation now honors every frame's base phase (not just
frame zero). Regenerate SpectralResynthParams and union/descriptors/WASM.

### Color Scan checkpoint

Appends ColorScan (34 kinds here). Image-only workflow shares bounded decoding
and cancellable worker preparation with resynthesis, but has distinct observable
hue/brightness/frequency/stereo mapping and a color stereo-width control. Written,
not executed: independent worker red/blue mapping, DSP measured frequency/pan,
interpolation and exact scan partition identity; engine scan history/disk/live
PCM; mounted image-only/width/history workflow. Regenerate ColorScanParams,
InstrumentKind/InstrumentParams, descriptors and simulator. All eleven rows now
have implementation candidates; runtime acceptance remains exclusively with the
integrator. Static cross-seam reconciliation follows.

### Static reconciliation / callback and preparation lifecycle checkpoint

Static review corrected duplicated MidiOut arms from the local registry edit
helper, restored the module-level documentation position and retained the exact
pre-stream registry order, appending seven distinct kinds once (ALL = 34).
The helper is not delivered. Updated the pinned display-name list and scalar
ParamSet contracts for all seven additions. Fixed an old MembraneParams test
literal for appended fields. Constructor validates external asset data before
attachment. Modal modes fade before their oscillator frequency clamp rather than
piling up at the clamp; audition buttons capture pointers and stop on pad changes.
Per-key queued edits recover/report dispatch failures.

Written, not executed: `dsp/realtime.rs::prepared_i3_i4_assets_events_extremes_and_reset_never_allocate_or_free`
loads real assets for all appropriate kinds, exercises extreme automation/note
storms/reset and counts alloc/zeroed/realloc/free with the existing TLS allocator.
`asset-job.test.ts` covers buffer transfer, success retirement, cancellation,
late results, timeout, worker error and pre-cancel refusal. Worker dispatch throws
now terminate/clear the job. Add asset-job.test.ts to the frontend test commands.

### Instrument preset asset checkpoint

Static inspection found channel presets already clone the whole source, while
instrument-only PluginPresetState previously captured just Copy parameters.
It now carries optional, omitted-when-absent immutable assets; capture/validate/
expected-destination comparison/apply/history preserve or explicitly reset them.
A preset load replaces both parameters and its asset atomically, including across
instrument kinds. Legacy plugin preset JSON remains valid. Written, not executed:
`instrument_plugin_presets_capture_replace_validate_and_undo_immutable_data`.
Additional shared seams: `project/src/preset.rs`, `lower/preset_apply.rs`.
Regenerate PluginPresetState (and its referenced command/preset types) too.

### Prepared voice continuity checkpoint

Prepared spectral voices now retain stereo endpoints with bounded 4 ms
corrections when stolen, preserving the hard 16-voice limit without an abrupt
cut to zero. Reset clears all correction state; stop keeps the existing fade.
Written, not executed: `prepared_voice_stealing_preserves_endpoint_and_stop_reaches_exact_silence`.
Run alongside the additive/allocator suites; existing independent single-voice
references remain untouched when no voice has been stolen.

### Sample-layer continuity checkpoint

Layer Kick's fixed eight sample voices retain a 4 ms endpoint correction on
steal, including a changed velocity/start position, and keep a short expired
sample alive only until that correction settles. Stop/reset clear it deterministically.
Written, not executed: `stolen_sample_layer_preserves_endpoint_and_stop_silences_corrections`;
existing independent sample-only playback remains unchanged without a steal.

### Spectral phase-wrap checkpoint

Phase interpolation now follows the shortest cycle path across zero and timed
scan interpolates the last column back into the first column. This avoids a
spurious half-cycle inversion for analyzed phases near 0/1 and a hard spectrum
jump on image scan wrap. Written, not executed:
`snapshot_phase_interpolation_takes_the_short_path_across_cycle_zero`.

### Mounted preparation/history checkpoint

All preparation editors cancel on history navigation as well as project
replacement/unmount. An Undo cannot accept an older worker job after an ABA
source restoration. Cancel also restores editor usability; null dispatch results
show a retry message (dispatch already reports the backend error). Written, not
executed: mounted full-width patch load/undo, snapshot harmonic load/undo,
worker refusal leaving history unchanged, and undo cancellation ignoring a late
result. These replace only the worker boundary in mounted tests; pure preparation,
worker retirement and real simulator/document mutation are tested separately.

Mounted cancellation assertion accepts both a retained instrument and its removal by Undo; neither can receive the late asset. Written, not executed.

### Import routing checkpoint

Narrow I3/I4 import-name mappings now select the full workflows: harmonic filter,
snapshot morph, metal voice, spectral resynth, number voice, color scan; Drumpad
selects the single Membrane and BassDrum gets LayerKick. Fruity Kick remains core
Kick. Existing opaque-state/case/default/loss-note tests updated, **written, not
executed**. This recognizes the workflow and does not recover proprietary patch
state or embedded sample/image sources. Shared additional seam:
`windfall-flp/src/convert/synth.rs`; run `cargo test -p windfall-flp --lib convert::synth`.
