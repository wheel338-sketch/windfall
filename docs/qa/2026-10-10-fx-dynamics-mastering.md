# Dynamics, transient and mastering effects â€” 2026-10-10

Stream: roadmap family E2 plus local mastering from X4. Branch `worktree-agent-a3639c5d4bc473d97`.

**Nothing in this document was compiled or executed.** The stream was edit-only. Every test named below is "written, not executed". The parent builds, regenerates and runs.

This file is kept current while the work proceeds, so an interruption leaves a usable hand-off. Sections marked *pending* are not written yet.

## Status by row

| Row | State | Summary |
| --- | --- | --- |
| `wf-cloud-mastering` | in progress | Loudness and true-peak measurement written (`mastering/loudness.rs`). Chain, export hook and review UI pending. |
| `fx-emphasis` | pending | New multistage Mastering Limiter kind. |
| `fx-emphasizer` | pending | Existing Stage Stack; editor and acceptance tests. |
| `fx-fruity-multiband-compressor` | pending | Existing DSP; meter, editor, acceptance tests. |
| `fx-maximus` | pending | Existing DSP; meter, editor, acceptance tests. |
| `fx-soundgoodizer` | pending | Existing DSP; one-knob editor, acceptance tests. |
| `fx-transient-processor` | pending | Existing DSP; split listen output, editor, tests. |
| `fx-low-lifter` | pending | Existing DSP; editor, acceptance tests. |

## What exists so far

### Loudness and true peak (`crates/windfall-dsp/src/mastering/loudness.rs`)

- `k_weighting(sample_rate)`: the two BS.1770-4 sections, derived from the analog prototype so they hold at every rate.
- `LoudnessMeter`: streaming stereo meter. Integrated loudness with absolute (-70 LUFS) and relative (-10 LU) gates, momentary and short-term maxima, loudness range (EBU Tech 3342), sample peak and 4x true peak.
- `PeakInterpolator`: twelve-tap Lanczos read at quarter-sample points. Six samples of delay. Reused by the limiter's detector.
- `measure(interleaved, sample_rate)`.

Unit tests in the same file, written, not executed:

- `the_48_khz_weighting_is_the_table_of_the_standard`
- `the_interpolator_reads_samples_exactly_and_constants_as_themselves`

Least sure: the 48 kHz table comparison uses a 1e-6 tolerance against constants recalled from the standard and from libebur128. If it fails by a small margin the derivation is still right and the tolerance is the thing to look at; if it fails by a lot, a constant is wrong.


## Continuation — 2026-10-11

Merge `feat/parity-100` completed at `67c7096f`, preserving playlist export,
mastering controls, tuner and both new module registrations. Incoming generated
artifacts belong to the integration merge; this worker has not edited them.
The earlier status table is stale: the previous worker also committed the chain,
export hook, seven purpose-built editors, DSP tests, mounted editor tests and
project/history tests. The following continuation entries supersede that table.

### `fx-transient-processor` — full-row candidate, pending integrator execution

Transient Shaper now advertises the same two auxiliary outputs as Transient
Split. Its shaped attack/sustain use `AnyEffect::process_aux`, the existing
prepared mixer buffers, `Mixer.effectOutputs` and `SetEffectOutput`. Routed
parts leave the source chain; unrouted parts stay. Listen auditions the retained
chain only, leaving routed parts independently audible. No new command or
callback storage was needed. The existing inspector supplies both routing menus.

Written, not executed:
- DSP `shaper_saved_aux_routes_remove_only_the_routed_parts_and_ignore_listen`:
  all four masks, all three listen settings, stereo polarity and 1/257-frame partitions.
- Project `shaped_transient_routes_are_saved_undoable_pruned_and_checked_for_cycles`:
  actual file reopen, undo/redo, rejected cycle atomicity, deletion/pruning/undo.
- Mounted `effect-outputs.qa.test.tsx` now exercises both transient kinds through
  actual menus, saved Rust document, target deletion and effect deletion/history.
- Existing `dynamics.test.tsx` covers the attack/sustain editor and automation.

Regenerate descriptors (Transient Shaper effectOutputs), Rust-derived TS and
WASM before frontend execution. Run the dynamics DSP/project tests and mounted
routing suite listed in the final command section. Engine audio acceptance is
being added below. No tests or builds have run in this worker.
