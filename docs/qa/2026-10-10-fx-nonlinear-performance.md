# Nonlinear, guitar and buffered performance effects — hand-off

Branch `worktree-agent-a3f88402660322a72`, branched from `feat/parity-100` at `fd60cdde`.

**Nothing in this document has been compiled or run.** The stream was edit-only by instruction. Every test listed is "written, not executed". The parent builds, tests and fixes after merging.

Rows: `fx-fruity-blood-overdrive`, `fx-fruity-waveshaper`, `fx-hardcore-11-guitar-fx`, `fx-distructor`, `fx-gross-beat`, `fx-transporter`, `fx-effector-12-fx`, `fx-fruity-scratcher`.

## Status at a glance

This table is updated at every commit. "Not started" means the row is still exactly as `fd60cdde` left it.

| Row | Effect kind | DSP | Persistence / engine | Editor | Tests written | State |
| --- | --- | --- | --- | --- | --- | --- |
| `fx-fruity-blood-overdrive` | `overdrive` | not started | — | — | — | not started |
| `fx-fruity-waveshaper` | `waveshaper` | not started | — | — | — | not started |
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

Filled in when `crates/windfall-dsp/src/asset.rs` is committed.

## Per-row detail

Filled in row by row as each is committed.

## Shared seam files touched

Updated at every commit.

| File | Change |
| --- | --- |
| — | none yet |

## What the parent must regenerate

- `apps/desktop/src/bindings/*` through `scripts/gen-bindings.sh` (parameter structs, `descriptors.json`, `automation-fixtures.json`).
- `apps/desktop/src/lib/ipc/sim/windfall_sim.wasm` and its metadata through `scripts/build-sim.sh`.

## Commands for the parent

Filled in as tests are written.

## Least-sure spots

Filled in as code is written.

## Remaining gates

Filled in at wrap-up.
