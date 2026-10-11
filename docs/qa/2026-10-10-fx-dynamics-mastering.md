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


### `fx-fruity-multiband-compressor` — full-row candidate

Retained the three independent peak-detector bands, compensated LR4 crossover,
external matched-band key, meter and purpose-built crossover/transfer editor.
Added independent f64 hard-knee/attack/release reference trajectories at three
rates: `three_detectors_match_independent_db_ballistics_without_cross_band_pumping`.
Each detector transitions at different frames; stereo link and unaffected other
band state are measured. Existing crossover magnitude, external-key, partition,
finite-audio, editor and persistence/history tests remain in place.
Added public engine acceptance module and registered it in `engine/main.rs`:
`multiband_compressor_automation_is_audible_and_matches_live_render_without_allocations`
measures audible automation, live/offline agreement after PDC, 17/251-frame
partitions, finite samples, tail decay and zero callback allocations/frees.
All tests written, not executed. Crossover phase reference follows below.


### `fx-maximus` — full-row candidate

Kept independently editable three-band compression, per-band limiters and sum
limiter, matched external key and dedicated split/curve/band/ceiling editor.
All reused LimiterParams literals retain `..LimiterParams::default()` so the
integration branch's mode/compression/gate fields default correctly.
Written, not executed: `maximizer_band_ceilings_protect_each_band_before_the_master_sum`
checks separate low/mid/high ceilings on far-band probes and 96-frame latency;
`unity_band_sum_matches_the_independent_cascaded_allpass_phase_reference` checks
both unity magnitude AND complex phase at three rates and both crossover points.
The latter also covers compressor recombination. Engine
`maximizer_band_automation_matches_live_render_with_both_pdc_stages` uses real
saved automation, renders/live comparison, partitioning, allocation guard and tail.
Existing DSP independent detectors, external key, curves, partitions; mounted
editor controls/automation; file/history tests apply to this distinct kind.
This effect promises sample ceilings, not calibrated true-peak limiting; the
Mastering Limiter and local mastering chain supply the true-peak workflow.


### `fx-soundgoodizer` — full-row candidate

The constrained One Knob workflow retains exactly one saved/automatable control
and a separate editor, backed by its own fixed multiband compression/limiting
recipe. Written, not executed:
`one_knob_is_constrained_but_still_has_a_real_ceiling_latency_and_finite_tail`
checks descriptor count, impulse onset after both look-aheads, decay, reset and
sample ceiling for off/half/full amounts. Existing
`the_one_knob_enhancer_gets_louder_all_the_way_up` measures actual calibrated
programme loudness. Engine
`one_knob_amount_is_audible_and_matches_live_render_without_callback_allocations`
covers saved amount automation, RT guard, PDC, live/export and partitioning.
Mounted distinct one-knob control and document/file/history cases already exist.
Amount zero retains crossover phase and the fixed sample-safety limiter; it is
not a phase-transparent bypass. No true-peak promise is made for this effect.


### `fx-low-lifter` — full-row candidate

Retained low-band harmonic generation, DC removal, 2x oversampling and aligned
dry path, with dedicated cutoff/drive/amount editor. Written, not executed:
`bass_octave_survives_fundamental_removal_and_amount_is_additive_after_aligned_dry`
measures the newly generated 120 Hz octave of a 60 Hz tone (absent in dry),
linearity of additive amount, even-harmonic polarity, removed DC and exact
13-frame dry alignment. Engine
`bass_harmonic_amount_automation_matches_live_render_with_compensated_latency`
adds RT/automation/live/export/partition/tail evidence to existing DSP spectrum,
partition, finite-input and mounted/persistence tests. No alias-free claim.


### `fx-emphasis` — full-row candidate

Previous work supplies a distinct Mastering Limiter: slow shared RMS leveler,
DC-filtered saturation/color, 4x-detected look-ahead peak stage, stage meters,
fixed PDC and dedicated ordered-stage editor. Added written, not executed:
`mastering_limiter_true_peak_is_checked_by_a_different_dense_reconstructor`
uses an independent 16x Hann/sinc reconstructor at 44.1/48/96 kHz;
`slow_leveling_keeps_shared_stereo_gain_and_tonal_balance` measures linked
stereo gain and three-tone balance with leveling active;
`mastering_limiter_gain_automation_matches_live_export_and_fixed_true_peak_latency`
adds native RT/automation/render/partition/tail acceptance (78 frames at 48 kHz).
Existing tests measure stage workload, calibrated level, true-peak on/off,
latency, live parameter invariance, persistence and mounted ordered controls.
True-peak reconstruction is an estimate: moving gain has a documented 0.3 dB
DSP tolerance. The offline mastering chain remeasures/corrects to its tighter
requested ceiling tolerance. Stationary analytic sine cases are separate.


### `fx-emphasizer` — full-row candidate

Stage Stack exposes the larger limiter's saturation curve plus an intentionally
simple immediate stereo-linked safety stage, separate from the three-stage
workflow. Dedicated two-stage editor, meter, drive/color/ceiling/mix and saved
history/automation were committed by the previous worker. Added written, not
executed: `simplified_two_stages_match_independent_saturation_and_linked_release_reference`
checks sample-by-sample f64 saturation, linked attack/recovery and dry/wet against
an independent formula; `two_stage_drive_automation_matches_live_export_without_delay_or_callback_allocation`
covers native DSP hosting and saved automation. Existing eight stage unit tests
cover reset, extreme inputs, live control ramps, instant lowered ceiling and
zero delay/tail. No true-peak claim; ceiling protects wet samples only.
