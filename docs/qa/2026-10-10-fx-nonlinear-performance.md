# Nonlinear, guitar and buffered performance effects — hand-off

Branch `worktree-agent-a3f88402660322a72`. Resumed implementation starts at the
merged `feat/parity-100` checkpoint `8e946db2`; the earlier DSP notes below are
retained as design history. Latest checkpoint: MIDI trigger completion, 2026-10-11.

**Nothing in this document has been compiled or run.** The stream was edit-only by instruction. Every test listed is "written, not executed". The parent builds, tests and fixes after merging.

Rows: `fx-fruity-blood-overdrive`, `fx-fruity-waveshaper`, `fx-hardcore-11-guitar-fx`, `fx-distructor`, `fx-gross-beat`, `fx-transporter`, `fx-effector-12-fx`, `fx-fruity-scratcher`.

## Status at a glance

Every row is a **full-row candidate**, pending integrator execution and listening.
All eight have purpose-built editors and written DSP/project/engine/mounted UI
acceptance. This is an implementation claim, not passing-test evidence.

| Row | Effect kind | Implemented distinct behavior | Remaining gates |
| --- | --- | --- | --- |
| `fx-fruity-blood-overdrive` | `overdrive` | Oversampled asymmetric guitar drive, pre-band, hot mode and tone controls. | Transfer/harmonic/alias tolerances, live/offline and editor checks. |
| `fx-fruity-waveshaper` | `waveshaper` | Freely drawn bent transfer curve, immutable lookup table, bipolar mode, legacy automatable knots. | Curve editor, alias/headroom and shape-change crossfade checks. |
| `fx-hardcore-11-guitar-fx` | `guitarRack` | Eleven simultaneous distinct pedals and four original synthetic cabinet IRs. | Registered inherited guitar tests, FIR/reference tolerances and maximum-rate CPU/listening. |
| `fx-distructor` | `driveChain` | Three reorderable distortion/filter/chorus/cabinet modules. | Order/reference/latency tests and worst-case CPU. |
| `fx-gross-beat` | `volumeGate` | Drawn time/volume replay and MIDI-linkable capture/restart/hold/release. | Exact host/recorded-trigger capture tests, curve editor and real-device MIDI/listening. |
| `fx-transporter` | `timeTransport` | Live onset detection, tempo-sized capture/reloop and MIDI-linkable manual triggering. | Musical detector tuning, onset/recorded-trigger tests, reset CPU and real-device MIDI/listening. |
| `fx-effector-12-fx` | `performanceRack` | Twelve actual treatments with independent XY steering and grouped history. | XY tests, partition checks and listening. |
| `fx-fruity-scratcher` | `scratch` | Loaded project-source manual/drawn scratching with immutable audio ownership. | Reload/persistence/editor tests, missing-file/budget UX, listening; preset sample portability belongs to its subsystem. |

## Design decisions that hold for every row

These were settled from reading the source before any edit. They explain why the diff looks the way it does.

1. **No new effect kinds.** All eight rows already have an `EffectKind`, an `EffectParams` variant and an `AnyEffect` arm. The work extends the eight existing processors in `crates/windfall-dsp/src/drive/` and `crates/windfall-dsp/src/performance/`. The registry in `effect.rs` is touched only for latency arms of these kinds and one forwarded transport method.
2. **Descriptor indices never move.** New controls are appended after the existing ones of each kind. Choice lists only gain options at the end. Automation that addresses an old index still reaches the same control.
3. **Old projects load.** Every parameter struct keeps `#[serde(default)]` at the container, so a project saved before a field existed loads with that field's default. Defaults are chosen so an old project sounds as it did, apart from the oversampling noted per row.
4. **Immutable assets, not big `Copy` structs.** Drawn curves and loaded scratch sources live in `windfall_dsp::asset`, an append-only, budgeted store that the audio thread reads without locking. A parameter struct holds an 8-byte reference. See "Asset contract" below.
5. **Constant latency for oversampled drive.** The oversampler delays by 32 samples at every setting, including "off", so delay compensation never changes while a setting is automated.
6. **Song position reaches built-in effects.** `Effect::set_transport(playing, beats)` is a new default-no-op trait method. The engine already computes a `PluginTransport` once per block for hosted plugins; `EffectUnit::plugin_transport` now forwards it to built-in effects too.

## Asset contract

`crates/windfall-dsp/src/asset.rs` (new, `pub mod asset`). Its module comment is the contract; in short:

- `CurveRef` and `ShapeRef` are 8-byte `Copy` references to a list of at most 64 `CurvePoint { x, y, tension }`. `ShapeRef` also carries a 2,049-entry table rendered when it is published.
- A reference serializes **as its points** and deserializes by publishing them. So a drawn curve is saved inside the effect's settings JSON, travels through the existing `setEffectParams` command, is undone by the existing history, and is duplicated with the effect, all with no change to the project model, commands, edits or patches. The frontend sees `CurvePoint[]`.
- Ids are a 64-bit FNV-1a hash of the cleaned, sorted points, so equal curves are one asset.
- Publishing locks a mutex that only publishers take. Lookup (`CurveRef::asset`, `asset::source`) reads `OnceLock` slots: no lock, allocation or wait. A found asset is `&'static`.
- Scratch saves a project sample id, but its prepared `SourceRef` identifies the
  decoded audio allocation. `asset::publish_audio(&AudioBuffer)` deduplicates
  shared allocations off-thread and returns a static immutable audio handle.
  Reused ids in other projects cannot redirect it. The legacy keyed publication
  API remains available; the loaded Scratch callback does not use it.
- Bounds: 32,768 curves, 64 MiB of curve data, 1,024 source entries, 1 GiB of source audio. Past a bound `publish` returns `AssetError::Full`, which a command reports as "the effect asset memory is full; restart to free it". `asset::stats()` exposes the counts.

**Deliberate limit:** the store only grows. Nothing is reclaimed until the process exits. This is what lets lookups be free of reference counting and lets undo history hold any id. An editor should commit a curve on pointer release, not on every pointer move, so an edit costs one asset. Reclaiming at project replacement is a remaining gate.

**Known wart:** `ParamSet::approach` generated by `param_set!` does not copy fields that are not controls, so it leaves a `shape` or curve reference as it was. No processor here relies on `approach` for those fields.

## Per-row detail

### Shared: oversampling (`drive/oversample.rs`, new)

`Oversampling` is `off | x2 | x4 | x8`, default `x4`. The kernel is the Blackman-windowed sinc `distortion.rs` already uses, scaled so its length is `32 * factor + 1` taps; the delay is therefore 32 samples at every factor. "Off" delays 32 samples and shapes at the host rate. Kernels are built once per process in `prepare` and shared. A quality change clears the high-rate memory and fades in over 96 samples.

Tests in `drive/oversample.rs` (written, not executed): `every_quality_delays_by_exactly_the_reported_latency`, `an_identity_shape_passes_the_audible_band_at_unity`, `oversampling_removes_the_aliases_a_soft_clipper_makes_at_the_host_rate`, `a_bounded_shape_overshoots_its_ceiling_by_a_bounded_amount`, `changing_quality_mid_stream_stays_finite_and_fades`, `the_plain_delay_matches_the_oversampler`, `unprepared_processing_is_the_host_rate_path`.

Tests in `asset.rs` (written, not executed): `equal_points_are_one_asset_and_different_points_are_not`, `points_are_cleaned_and_sorted_before_they_are_stored`, `a_curve_interpolates_holds_its_ends_and_jumps_where_points_share_an_x`, `tension_bends_a_segment_without_moving_its_ends`, `a_shape_table_follows_the_curve`, `a_reference_is_saved_as_its_points_and_loads_back_equal`, `a_source_is_found_by_key_and_the_latest_audio_wins`, `readers_see_whole_assets_while_another_thread_publishes`.

### `fx-fruity-blood-overdrive` — Overdrive

Implemented in `drive/overdrive.rs`:

- The clipper now runs through the oversampler. Latency is 32 samples (was 0).
- New controls appended at indices 5, 6, 7: `preBandHz` (200–4000 Hz, default 900, which was the fixed value), `hot` (another 20 dB into the clipper, default off), `oversampling`.
- An old project loads with those defaults and sounds as before apart from the oversampling filter and the 32-sample delay.

Tests in `drive/shaping_tests.rs` (written, not executed): `the_overdrive_curve_matches_its_description_and_hot_adds_twenty_decibels`, `overdrive_asymmetry_adds_even_harmonics_and_matches_the_curve`, `the_pre_band_sets_where_the_emphasis_starts`, `old_overdrive_settings_load_with_the_new_controls_at_their_defaults`, and its halves of `oversampling_lowers_the_aliasing_of_a_drawn_clip_and_of_the_overdrive` and `a_bounded_curve_keeps_its_headroom_at_every_quality`. Existing tests in `drive/tests.rs` were updated for the new fields.

The purpose-built editor and project/engine/mounted acceptance were completed
in resumed checkpoint 1 below; all tests remain unexecuted.

### `fx-fruity-waveshaper` — Waveshaper

Implemented in `drive/waveshaper.rs`:

- The 33 knots and both gains keep indices 0–34. Appended: `oversampling` (35), `mix` (36), `bipolar` (37).
- New field `shape: ShapeRef`, not a control: a freely drawn curve of up to 64 points with per-segment tension. When present it replaces the knots. Bipolar reads it as input −1..1 against output −1..1; unipolar reads it for the magnitude and mirrors it.
- The curve runs through the oversampler; latency is 32 samples (was 0). The dry share of `mix` is delayed to match.
- Replacing the shape, or switching `bipolar`, crossfades between the two curves over 5 ms.

Tests in `drive/shaping_tests.rs` (written, not executed): `a_drawn_shape_is_the_transfer_curve_in_both_readings`, `harmonics_of_a_drawn_clip_match_the_curve_and_follow_its_symmetry`, `mix_blends_with_the_delayed_input_and_a_shape_edit_crossfades`, `old_waveshaper_settings_load_and_a_shape_is_saved_as_its_points`, plus the shared aliasing and headroom tests above. `identity_waveshape_is_unity` and `bent_curve_changes_amplitude_and_interpolates_knots` in `drive/tests.rs` were rewritten for the latency.

The drawn-curve editor and project/engine/mounted acceptance were completed
in resumed checkpoint 1 below; all tests remain unexecuted.

## Shared seam files touched

Updated at every commit.

| File | Change |
| --- | --- |
| `crates/windfall-dsp/src/effect.rs` | GuitarRack/DriveChain latency and default/forwarded built-in transport method. Overdrive/Waveshaper latency and lib.rs asset declaration are inherited. |
| `crates/windfall-dsp/src/drive/mod.rs` | Cabinet module/export and guitar rack test registration. |
| `crates/windfall-dsp/tests/dsp/{main,slot}.rs` | Acceptance module registration; GuitarRack/DriveChain added to expected latency kinds. |
| `crates/windfall-engine/src/{controller,plan,project_preparation,rack}.rs` | Prepare immutable Scratch audio handles/refuse asset budget failures; forward transport to built-ins. |
| `crates/windfall-engine/tests/engine/main.rs` | Nonlinear/performance acceptance module registration. |
| `crates/windfall-project/src/{check,lower}.rs` | Validate Scratch source ids and refuse removal of used sources. |
| `apps/desktop/src/features/effects/effect-editor.tsx` | Eight purpose-built editor registrations. |
| `apps/desktop/src-tauri/src/session/tests/mod.rs` | Native MIDI-link acceptance module registration only; controller runtime is reused. |

## What the parent must regenerate

- `apps/desktop/src/bindings/*` through `scripts/gen-bindings.sh` (parameter structs, `descriptors.json`, `automation-fixtures.json`).
- `apps/desktop/src/lib/ipc/sim/windfall_sim.wasm` and its metadata through `scripts/build-sim.sh`.

## Commands for the parent

In Git Bash after `source scripts/msvc-env.sh`, with `TS_RS_EXPORT_DIR` pointing at a temporary directory:

```bash
cargo test -p windfall-dsp --lib asset
cargo test -p windfall-dsp --lib drive
cargo test -p windfall-dsp --lib effect::registry_tests
cargo test -p windfall-dsp --test dsp
```

## Least-sure spots

Ordered by how likely I think each is to need a fix.

1. **Numeric thresholds in the new tests.** Every level, ratio and tolerance was reasoned out, not measured. The ones with the least margin: the 2% harmonic match in `harmonics_of_a_drawn_clip_match_the_curve_and_follow_its_symmetry`; the `naive / 16.0` alias bounds; the `0.9..=1.25` headroom window; the `2.0e-3` identity bound in `identity_waveshape_is_unity`.
2. **`#[ts(as = "Vec<crate::asset::CurvePoint>")]` on `WaveshaperParams::shape`.** Field-level `ts(as)` is used elsewhere in the crate for arrays; I have not seen it used here with a path to another module's type.
3. **`param_set!` with a `choice` row whose enum lives in another module** (`Oversampling` in `overdrive.rs` and `waveshaper.rs`). The macro writes `$enum::$variant`, so the enum must be imported by name, which it is.
4. **Latency moved from 0 to 32 for two kinds.** `effect.rs` and `tests/dsp/slot.rs` are updated. Any test elsewhere that assumes these two effects are sample-aligned pass-throughs would now fail; I found none by search.
5. **`asset.rs` statics.** `static WRITER: Mutex<Usage>` and `OnceLock<Box<[OnceLock<Box<CurveAsset>>]>>` rely on const `Mutex::new` and on `OnceLock::get` returning `&'static` for a static; both are stable, but none of it has met the compiler.

## Remaining gates

Filled in at wrap-up.

## Resumed implementation — 2026-10-11, checkpoint 1

The inherited hand-off above describes an earlier checkpoint and is not current
completion evidence. HEAD on arrival was `8e946db2`; a checkpoint also contains
an unfinished eleven-pedal guitar implementation and synthetic cabinet source.
The guitar module does not yet declare its cabinet sibling. This is being
completed in this stream, not treated as a passing integrated feature.

- Overdrive: purpose-built editor exposes all eight existing controls. Full-row
  candidate after integration checks; no new DSP claim in this checkpoint.
- Waveshaper: mounted-purpose curve editor supports adding/moving up to 64
  points, bending segments, precise numeric edits, deletion, reset, keyboard
  arrows, and draft cancellation on Escape/capture loss/focus loss. Pointer
  moves remain local; release sends one checked `SetEffectParams`. Commit reads
  the current slot to preserve concurrent scalar changes.
- Tests **written, not executed**: `crates/windfall-project/tests/nonlinear_performance.rs`
  (`settings_gestures_noops_and_disk_save_reopen_preserve_every_control`,
  `invalid_edits_are_atomic_and_finite_extrema_use_descriptor_sanitization`,
  `every_control_has_the_correct_automation_range_and_removal_history_restores_curves`,
  `old_v1_parameters_and_drawn_shapes_roundtrip_with_exact_history`);
  `crates/windfall-engine/tests/engine/nonlinear_performance.rs`
  (`all_eight_processors_capture_the_same_pcm_live_offline_and_under_block_partitions`,
  registered in engine/main.rs); `nonlinear/curve-editor.test.tsx` (release-only
  publication, cancellation, keyboard and bend edits).
- Integrator: regenerate bindings/descriptors and WASM first. Then run
  `cargo test -p windfall-project --test nonlinear_performance`,
  `cargo test -p windfall-engine --test engine nonlinear_performance`,
  `pnpm --dir apps/desktop test src/features/effects/nonlinear --maxWorkers=4`.
- Shared seams added: effects/effect-editor.tsx editor registrations;
  engine/tests/engine/main.rs test module registration. Generated CurvePoint
  and widened Overdrive/Waveshaper parameter bindings are required.
- Risks: source inspection found an unfinished inherited guitar module that
  cannot compile until checkpoint 2; verify regenerated TS curve field shape,
  jsdom pointer support, inherited shaping thresholds, and engine render
  sample-rate defaults. Asset store remains bounded and append-only.

## Checkpoint 2 — Guitar Rack

- Completed the inherited eleven-pedal DSP wiring: declared/exported cabinet
  module and pedal types; reported constant 32-frame drive latency in the
  effect registry and slot tests; repaired widened old test literals and
  spring timing expectations. Cabinet response generation now handles even
  pathological sub-3 Hz prepare rates without a filter clamp panic.
- The eleven switches and two independent knobs per pedal are exposed in a
  purpose-built pedalboard editor, with documented fixed signal order,
  cabinet selection and oversampling. The old `model` remains a compatibility
  pedal enable; select Clean boost for switch-only operation.
- Four own cabinet IR recipes are generated in `drive/cabinet.rs` at prepare,
  never on process/set/reset. Provenance is fully documented there: synthetic
  filter excitation, baffle reflections and deterministic noise; no recorded
  or proprietary IR. Max 1,024 taps per cabinet, four banks per processor.
- Full-row candidate only after integrator checks. Tests **written, not
  executed**: `drive/guitar_rack_tests.rs` series-rack/control independence,
  block invariance, legacy/index/persistence and memory/headroom cases;
  inherited cabinet tests direct FIR reference, deterministic unit energy,
  response distinction and treble rejection are now registered and reachable.
- Integrator: `cargo test -p windfall-dsp --lib drive`,
  `cargo test -p windfall-dsp --test dsp`, plus checkpoint 1 commands.
  Regenerate PedalParams, Cabinet and widened GuitarRackParams.
- Risk order: inherited octave harmonic/gate/compressor numerical tests now
  run against the expanded pedalboard; cabinet response thresholds; direct
  1,024-tap FIR CPU at high rates (no performance measurement was executed).
  Existing callback guards will exercise prepared cabinet processing.

## Checkpoint 3 — Drive Chain

- Three reorderable modules now independently select bypass, asymmetric drive,
  cubic shape, fuzz, resonant lowpass/highpass, stereo modulated chorus, or one
  of four synthetic cabinets. Existing model/gain descriptor pairs and output
  index 6 remain stable; all filter/chorus/cabinet/quality controls append.
- Every stage has a constant 32-frame oversampled path (including bypass), so
  total latency is 96 frames and quality/model automation does not reallocate.
  Module weights ramp in 5 ms; cabinet changes crossfade in 5 ms. Filter,
  chorus and cabinet histories run continuously. The stage's `gainDb` remains
  the drive gain for the three nonlinear modes, preserving its old meaning.
- Purpose-built editor shows the ordered modules and applicable controls;
  Move earlier swaps complete module settings in one undoable command.
- Tests **written, not executed**: `drive/chain.rs::tests` actual filter,
  chorus/cabinet participation, ordering, partition invariance, old descriptor
  identities and independently written static transfer reference. Inherited
  chain tests adjusted for widened settings and constant latency.
- Candidate after verification. Commands: `cargo test -p windfall-dsp --lib drive`,
  `cargo test -p windfall-dsp --test dsp`; checkpoint 1 project/engine/UI commands.
- Regenerate DriveStage, DriveStageParams, DriveChainParams and descriptors.
  Risks: new multi-stage resampler reference tolerances; three FIR cabinets
  plus three oversamplers worst-case CPU; numerical thresholds remain unmeasured.

## Checkpoint 4 — buffered performance and loaded scratching

All four buffered processors now have purpose-built editors and persisted
assets. Every test below is **written, not executed**; no build, package runner,
compiler check, benchmark or test was invoked by this worker.

| Row | Implementation and current claim | Remaining gates |
| --- | --- | --- |
| `fx-gross-beat` | Full-row software candidate: immutable drawn time and volume curves over a bounded stereo replay ring, segment bends/steps, curve editor, host position sync and seek crossfades. MIDI completion below adds capture/restart/hold/release. | Integrator checks, tempo-map/seek listening, real-device MIDI validation. |
| `fx-transporter` | Full-row software candidate: dual-envelope onset detector; threshold/contrast/holdoff; captures from detected onset for a tempo-derived slice and reloops/reverses/holds; manual trigger retained. | Detector thresholds on musical material, callback time at maximum rate/reset, MIDI acceptance execution/real-device validation. |
| `fx-effector-12-fx` | Full-row software candidate: twelve actual treatments, both independent XY axes have model-specific meanings, keyboard/pointer XY pad writes one grouped history gesture. | Integrator tests and performance/listening. Buffered pitch/speed paths interpolate linearly; no duration-preserving pitch/formant claim. |
| `fx-fruity-scratcher` | Full-row software candidate: chooses a loaded project source, manual platter position plus drawn position curve; immutable runtime audio identity prepared off-thread; mono/stereo fractional interpolation; exact source/history/file persistence. | Missing-file and budget-refusal native UX, loaded-source listening, preset portability of project sample references is a separate subsystem gate. |

Clock/ownership details:
- Added default `Effect::set_transport`; AnyEffect/EffectSlot forward it. The
  existing engine effect transport boundary now forwards song position to
  built-ins, while preserving native forwarding.
- Drawn replay uses at most a one-second slice with recording in the disjoint
  remainder of the two-second ring (6,144,000 bytes at 384 kHz). Legacy step
  gating retains its original two-second lookback.
- Transient capture uses TWO preallocated two-second rings: maximum 12,288,000
  bytes at 384 kHz. Onset resets write/filled metadata; completion swaps ring
  owners in constant time. No onset-sized PCM copies or allocations. Reset
  clears both rings in place, which still needs worst-case CPU measurement.
- Curves are immutable CurveRef/ShapeRef assets. Loaded scratch audio is now
  published as SourceRef by allocation identity, never project-local sample
  number. SourceRef is skipped in JSON/TS; sourceId is saved. Different projects
  sharing sourceId 1 cannot redirect one another. Pool publication shares PCM,
  and callback set/reset/process retain plain static references.
- Checked source edits refuse unknown sample ids; RemoveSample refuses sources
  used by Scratch. Missing decoded audio is silent on Scratch's wet path and
  the editor displays the existing sample-info error. Budget refusal is checked
  by preparation/readiness and detached installation and reports an explicit
  restart-to-release error. Asset storage is bounded, append-only until exit.

Tests written, not executed:
- `performance/extended_tests.rs`: `drawn_time_reverses_a_bounded_record_and_volume_follows_an_independent_line`,
  `host_position_sets_exact_phase_and_seeks_preserve_history`,
  `transient_capture_starts_at_the_onset_and_reloops_at_exact_tempo_length`,
  `both_xy_axes_change_each_of_the_twelve_independent_treatments`,
  `maximum_replay_storage_is_fixed_even_for_huge_host_blocks`,
  `loaded_scratch_interpolates_a_stereo_source_and_follows_reverse_curves`,
  `runtime_source_identity_cannot_leak_between_projects_that_reuse_an_id`,
  `deliberate_source_discontinuities_follow_a_two_millisecond_crossfade`.
- `tests/dsp/nonlinear_performance.rs` (registered in tests/dsp/main.rs): zero
  callback allocations AND frees while swapping real curve/source handles,
  triggering onset capture, changing XY and updating transport/reset.
- `drive/shaping_tests.rs`: `guitar_and_modular_fuzz_reject_folded_harmonics_with_oversampling`
  adds explicit alias-energy/headroom coverage for the other two E4 rows.
- Project acceptance adds drawn replay/scratch save/reopen, exact history,
  source deletion refusal. All eight rows share descriptor and automation tests.
- Engine acceptance now exercises actual drawn replay, transient capture,
  loaded scratch, XY reverse, simultaneous guitar pedals and a
  distortion/chorus/cabinet chain, rather than only neutral defaults. It compares
  public live/offline PCM under 1/7/64/101/137/512 partitions and guards callbacks.
- `nonlinear_performance.test.tsx`: all eight mounted editor registration,
  mixer mix/bypass/duplicate/replace/history and real-WASM descriptor coverage;
  drawn waveshaper persistence, grouped XY history and loaded scratch selection.
  `nonlinear/curve-editor.test.tsx`: drawing cancellation, release-only asset
  publication, bend/keyboard edits, independent XY movement/capture release.

Integrator commands (Git Bash after `source scripts/msvc-env.sh`, TS_RS_EXPORT_DIR
pointing at a task-specific directory):
```bash
cargo test -p windfall-dsp --lib asset
cargo test -p windfall-dsp --lib drive
cargo test -p windfall-dsp --lib performance
cargo test -p windfall-dsp --lib effect::registry_tests
cargo test -p windfall-dsp --test dsp
cargo test -p windfall-project --test nonlinear_performance
cargo test -p windfall-engine --test engine nonlinear_performance
```
Regenerate bindings/descriptors/automation fixtures and simulator before:
```bash
pnpm --dir apps/desktop test src/features/effects/nonlinear src/features/effects/nonlinear_performance.test.tsx --maxWorkers=4
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```
Then run the normal project/engine persistence, preset and realtime regressions,
scoped strict Clippy and freshness checks. No generated files were edited here.

New/widened generated types: CurvePoint, Cabinet, PedalParams, Oversampling,
OverdriveParams, WaveshaperParams, GuitarRackParams, DriveStage,
DriveStageParams, DriveChainParams, VolumeGateParams, TimeTransportParams,
ScratchParams, PerformanceRackParams. No new Command variant or IPC operation.

Shared seams additionally touched: DSP effect.rs, tests/dsp/main.rs;
engine controller.rs, plan.rs, project_preparation.rs, rack.rs;
project check.rs/lower.rs; effects/effect-editor.tsx. Model/command/IPC files,
parity tracker, generated bindings and WASM are unchanged.

Risk order for integration:
1. Compile widened params/TS derives and generated sourceId/curve array shapes.
   The inherited guitar implementation was unfinished on arrival; its formerly
   unregistered cabinet tests now run.
2. Numeric tolerances in inherited/new harmonic, alias, cabinet and onset tests
   are reasoned, not measured. Check failures without weakening reference assertions.
3. Engine block invariance under actual song-position updates, especially
   noninteger tempo/loop periods and pattern wrap. Seek retains ring history.
4. Mounted jsdom pointer/capture support, Base UI source picker role, and grouped
   asynchronous history dispatch with regenerated WASM.
5. CPU at maximum rate: direct cabinet FIRs (up to 1,024 taps), multiple
   oversamplers, ring clearing on reset. Allocation safety does not establish
   a deadline. Retained immutable source/curve budgets are process-wide and
   reclaim only on exit; no project-replacement reclamation is claimed.

## Final source-review corrections

- Curve deserialization now bounds its temporary vector at 64 points before
  publication; added `asset::tests::deserialization_rejects_more_than_sixty_four_points`
  (**written, not executed**).
- Added `engine::nonlinear_performance::scratch_source_reload_reuses_project_ids_without_redirecting_another_engine`
  (**written, not executed**) to cover pool replacement, retained independent
  owners and allocator-free adoption, beyond the pure-DSP source identity case.
- Restored access to the old automatable Waveshaper knots and Volume Gate steps:
  their editors can explicitly clear the drawn asset and expose the old controls.
- Pan Spin's Y-steered oscillator now keeps a continuous phase across slice
  boundaries, avoiding a new pan jump at noninteger orbit rates. Automatic
  transient capture warms up without delaying its initial live pass-through.
- Generic project acceptance correctly rounds integer/toggle gesture and
  automation midpoints; this was found by reading the real ParamKind contract.
- Integrator should run the same commands above. Only touched owned Rust files
  were rustfmt-ed; git whitespace inspection found no whitespace errors. No
  executable validation, tests or build commands were run.
- Full-row candidates are implementation claims only. The mapped MIDI-trigger
  gap identified here is addressed in the checkpoint below using the existing
  native controller link runtime. Its acceptance still needs execution.

## MIDI trigger completion — 2026-10-11

- Existing `ControlSource::Midi` / `MidiControl::Note` already maps key-down to
  1 and release to 0 through `session/controllers.rs`, with source/channel
  filtering, persisted checked commands and automation recording. Reused this
  accepted path rather than adding another MIDI event/model contract.
- Volume Gate now appends `trigger` at descriptor **21** (indices 0..20 retain
  their meanings). Legacy files default it to false. Like other descriptor
  controls it serializes explicitly: `readParam` and the JSON/descriptor contract
  require the path to exist. Default-project JSON is unchanged (no effect slots);
  no pinned per-effect default string was found. Rising edges
  capture preceding audio and restart both envelopes; a held trigger freezes
  input history so arbitrarily long relooping cannot overwrite the slice. Release
  resumes capture. Without a drawn time curve, the held record plays forward
  at unity. A retained clock offset preserves the restart across host block
  boundaries. Head transitions remain two milliseconds; held tail is infinite.
- The Volume Gate editor exposes Trigger with the existing parameter context
  menu's controller linking. Time Transport's existing Trigger uses the same
  menu and native map. No new Command, IPC operation, device code or dependency.
- Tests **written, not executed**:
  - `performance/extended_tests.rs::mapped_gate_trigger_restarts_curves_and_preserves_a_held_capture_across_host_blocks`
    verifies exact independent envelopes, frozen storage beyond ring capacity,
    host-offset preservation, 1/7/64/137/512 partitions, release and old indices.
  - `performance/extended_tests.rs::a_gate_trigger_without_a_drawn_time_curve_repeats_the_original_input_at_unity`.
  - DSP callback allocator test now toggles the actual Volume Gate trigger.
  - `engine/nonlinear_performance.rs::recorded_trigger_edges_replay_the_exact_history_in_live_and_offline_capture`
    compares both buffered effects against the original input slice and release,
    including all live block partitions. Edges sit on the engine's existing
    64-frame automation grid; off-grid recording retains that existing granularity.
  - `session/tests/nonlinear_performance.rs::midi_notes_capture_and_release_both_buffered_processors_through_saved_controller_links`
    uses production note mapping and pump, actual synth PCM capture/release,
    channel/key filtering, repeated-down no-op, save/open, removed-target undo
    and project replacement. It opens no hardware port.
  - Mounted `nonlinear_performance.test.tsx` adds both Trigger controls' actual
    commands, exact undo/redo and saved reopen.
- Additional integrator command (after the commands already listed):
  `cargo test -p windfall-desktop --lib session::tests::nonlinear_performance`.
  Regenerate widened VolumeGateParams/descriptors/automation fixtures/WASM.
- Check native-session capture/tail thresholds and startup/adoption timing,
  the recorded automation edge/reference assertions, then mounted Trigger roles.
  Real Windows MIDI pad/key timing, hotplug and listening remain external gates.
- Restart audit found the earlier worktree clean at `59cbfe90`; nothing was
  redone. No build, test, package runner, generator or real-device check was run.
