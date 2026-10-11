# Nonlinear, guitar and buffered performance effects — hand-off

Branch `worktree-agent-a3f88402660322a72`, branched from `feat/parity-100` at `fd60cdde`.

**Nothing in this document has been compiled or run.** The stream was edit-only by instruction. Every test listed is "written, not executed". The parent builds, tests and fixes after merging.

Rows: `fx-fruity-blood-overdrive`, `fx-fruity-waveshaper`, `fx-hardcore-11-guitar-fx`, `fx-distructor`, `fx-gross-beat`, `fx-transporter`, `fx-effector-12-fx`, `fx-fruity-scratcher`.

## Status at a glance

This table is updated at every commit. "Not started" means the row is still exactly as `fd60cdde` left it.

| Row | Effect kind | DSP | Persistence / engine | Editor | Tests written | State |
| --- | --- | --- | --- | --- | --- | --- |
| `fx-fruity-blood-overdrive` | `overdrive` | done | no model change needed | generic editor only | DSP yes; project, engine, UI no | DSP committed |
| `fx-fruity-waveshaper` | `waveshaper` | done | no model change needed (shape saved inline) | generic editor only; curve editor not written | DSP yes; project, engine, UI no | DSP committed |
| `fx-hardcore-11-guitar-fx` | `guitarRack` | not started | — | — | — | not started |
| `fx-distructor` | `driveChain` | not started | — | — | — | not started |
| `fx-gross-beat` | `volumeGate` | not started | — | — | — | not started |
| `fx-transporter` | `timeTransport` | not started | — | — | — | not started |
| `fx-effector-12-fx` | `performanceRack` | not started | — | — | — | not started |
| `fx-fruity-scratcher` | `scratch` | not started | — | — | — | not started |

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
- Scratch sources are keyed by the project's sample id. `asset::publish_source(key, &AudioBuffer)` stores a clone of the shared buffer; the newest audio for a key wins; `asset::source_epoch()` tells a processor to look again.
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

Not done: a purpose-built editor (it gets the generic one, which shows all eight controls), project-level and engine-level tests.

### `fx-fruity-waveshaper` — Waveshaper

Implemented in `drive/waveshaper.rs`:

- The 33 knots and both gains keep indices 0–34. Appended: `oversampling` (35), `mix` (36), `bipolar` (37).
- New field `shape: ShapeRef`, not a control: a freely drawn curve of up to 64 points with per-segment tension. When present it replaces the knots. Bipolar reads it as input −1..1 against output −1..1; unipolar reads it for the magnitude and mirrors it.
- The curve runs through the oversampler; latency is 32 samples (was 0). The dry share of `mix` is delayed to match.
- Replacing the shape, or switching `bipolar`, crossfades between the two curves over 5 ms.

Tests in `drive/shaping_tests.rs` (written, not executed): `a_drawn_shape_is_the_transfer_curve_in_both_readings`, `harmonics_of_a_drawn_clip_match_the_curve_and_follow_its_symmetry`, `mix_blends_with_the_delayed_input_and_a_shape_edit_crossfades`, `old_waveshaper_settings_load_and_a_shape_is_saved_as_its_points`, plus the shared aliasing and headroom tests above. `identity_waveshape_is_unity` and `bent_curve_changes_amplitude_and_interpolates_knots` in `drive/tests.rs` were rewritten for the latency.

Not done: the drawn-curve editor in the frontend. Until it exists the shape can only be set by a `setEffectParams` command; the generic editor shows the 33 knots and the three new controls.

## Shared seam files touched

Updated at every commit.

| File | Change |
| --- | --- |
| `crates/windfall-dsp/src/lib.rs` | one line, `pub mod asset;`, after `pub mod additive;` |
| `crates/windfall-dsp/src/effect.rs` | four arms changed in place: `EffectKind::{Waveshaper, Overdrive}` in `max_latency_samples` and `EffectParams::{Waveshaper, Overdrive}` in `latency_samples` now give `crate::drive::DRIVE_LATENCY_SAMPLES` |
| `crates/windfall-dsp/tests/dsp/slot.rs` | `Waveshaper` and `Overdrive` added to the `has_latency` list of `any_effect_reports_what_it_holds_and_refuses_other_settings` |

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
