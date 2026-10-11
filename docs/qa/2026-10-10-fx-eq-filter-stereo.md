# EQ, filter and stereo effects (family E1) — hand-off, 2026-10-10

Branch `worktree-agent-a1496919ce404b85b`, cut from `feat/parity-100` at `fd60cdde`.

**Nothing in this stream was compiled, run, linted or generated.** Every test named below is
"written, not executed". The parent builds, regenerates bindings and the simulator, runs the
tests and repairs what does not compile or pass.

Rows: `fx-fruity-7-band-eq`, `fx-spreader`, `fx-fruity-stereo-enhancer`, `fx-fruity-panomatic`,
`fx-equo`, `fx-fruity-love-philter`, `fx-fruity-filter`, `fx-fruity-parametric-eq`,
`fx-frequency-splitter`.

This file is updated after each row. A row that is not listed under "Row status" yet has not
been started.

## Row status

| Row | Processor | State |
| --- | --- | --- |
| `fx-fruity-7-band-eq` | Seven Band (`sevenBand`) | editor and tests written; candidate for done once they pass |
| `fx-spreader` | Spreader (`spreader`) | editor and tests written; candidate for done once they pass |
| `fx-fruity-stereo-enhancer` | Stereo Enhancer (`stereoEnhancer`) | phase offset and inversion added, editor and tests written; candidate for done once they pass |
| `fx-fruity-panomatic` | Pan Motion (`panLfo`) | shapes and tempo sync added, editor and tests written; candidate for done once they pass |
| `fx-equo` | Morph EQ (`morphEq`) | graphic curves, morph across up to four, editor and tests written; candidate for done once they pass |
| `fx-fruity-love-philter` | Filter Bank (`filterBank`) | full-row candidate; real eight-unit routing and drawn gate/cutoff envelopes; tests written, not executed |
| `fx-fruity-filter` | Selectable filter (`selectableFilter`) | full-row candidate; dedicated sweep editor and row-specific tests written, not executed |
| `fx-fruity-parametric-eq` | baseline EQ (`eq`) | full-row candidate for the seven-band summary; measured complex response and host/mounted cases written, not executed |
| `fx-frequency-splitter` | Band Split (`bandSplit`) | full-row candidate; integrated real low/mid/high mixer routes and editor; tests written, not executed |

## Test files (all written, not executed)

- `crates/windfall-dsp/tests/eq_filter_stereo.rs` — measured level and phase against analog
  prototypes written in the test, block invariance, bypass continuity.
- `crates/windfall-project/tests/eq_filter_stereo.rs` — every control as one undo step, file
  round trip, rejected edits, automation targets, type-only (legacy) settings.
- `crates/windfall-engine/tests/eq_filter_stereo_acceptance_qa.rs` — `accept()` drives each
  processor through native render, undo/redo, disk, live-versus-offline in three block sizes
  and a song-mode automation curve.
- `apps/desktop/src/features/effects/eq-filter-stereo.test.tsx` — mounted editors over the
  WASM document.

Commands for the parent (Git Bash, after `source scripts/msvc-env.sh` and a temporary
`TS_RS_EXPORT_DIR`):

```bash
cargo test -p windfall-dsp --lib eqbank
cargo test -p windfall-dsp --lib spatial
cargo test -p windfall-dsp --lib control
cargo test -p windfall-dsp --lib multiband
cargo test -p windfall-dsp --test eq_filter_stereo
cargo test -p windfall-dsp --test dsp
cargo test -p windfall-project --test eq_filter_stereo
cargo test -p windfall-engine --test eq_filter_stereo_acceptance_qa
cargo test -p windfall-engine --test engine realtime
# after scripts/gen-bindings.sh and scripts/build-sim.sh:
pnpm --dir apps/desktop test src/features/effects/eq-filter-stereo.test.tsx src/features/params --maxWorkers=4
```

## `fx-fruity-7-band-eq` — Seven Band

Implemented: the DSP was already registered (seven fixed bells, Q 1.4, 63 Hz to 16 kHz). Added
the purpose-built editor `features/effects/graphic/seven-band-editor.tsx` (seven vertical
faders bound and automatable like knobs, the response curve they make, a one-step "Flat"),
the shared `response/curve.ts`, `response/response-display.tsx` and
`controls/param-fader.tsx`, and registered it in `effect-editor.tsx`.

Tests: DSP `seven_band_matches_seven_fixed_bells_in_level_and_phase`,
`seven_band_centers_are_fixed_and_each_fader_moves_only_its_band`,
`seven_band_at_rest_costs_nothing_and_changes_nothing`,
`seven_band_is_block_invariant_and_bypasses_without_a_click`, and the ignored timing case
`seven_band_costs_less_than_the_parametric_equalizer_with_every_band_working`; engine
`seven_band_faders_reach_native_audio_history_files_and_automation`; project cases over
`KINDS`; UI `describe("Seven Band")` (three cases).

Least sure: the 0.05 dB / 0.01 rad tolerances of the measured comparison; `ParamFader`
passing `data-param` and hint props through `Fader`; whether `Fader` answers Home/End the way
a knob does (the UI helper `toEnd` relies on it).

Remaining gate: "low CPU cost" is shown structurally (flat bands run no filter, bit-exact
pass-through) and by an ignored wall-clock comparison; no CPU budget is asserted in the
ordinary run.

## `fx-spreader` — Spreader

Implemented: DSP unchanged (mid/side width plus a Haas difference added to one side and taken
from the other). New editor `features/effects/stereo/spreader-editor.tsx` with the width
picture `stereo/stereo-field.tsx` and the mono statement.

Tests: DSP `spreader_matches_a_mid_side_reference_and_folds_to_the_same_mono`,
`spreader_makes_a_mono_source_wide_without_changing_its_mono_sum`,
`spreader_is_block_invariant_and_bypasses_without_a_click`; engine
`spreader_controls_reach_native_audio_history_files_and_automation`; UI `describe("Spreader")`
(two cases); helpers `the stereo picture`.

Least sure: the 3e-6 tolerance of the sample-by-sample reference.

Remaining gate: none known beyond execution.

## `fx-fruity-stereo-enhancer` — Stereo Enhancer

Implemented (`crates/windfall-dsp/src/spatial/stereo.rs`): two appended controls,
`phaseOffsetMs` (-20 to 20 ms; above zero holds the right side back, below zero the left, with
linear interpolation below one frame) and `invert` (`off`/`left`/`right`, as a gain that glides
through zero). Descriptor indices 0 to 4 are unchanged; the new ones are 5 and 6. Zero latency
is kept: the offset is an audible effect, like the spreader's Haas delay. With both at their
defaults the output is bit-identical to before (the lines are written but not read, the signs
multiply by exactly 1). New types `StereoInvert` and `MAX_PHASE_OFFSET_MS` are exported from
`windfall_dsp::spatial`. Editor `stereo/stereo-enhancer-editor.tsx`.

This closes the "open phase gate" named in `docs/integration/seams/spatial.md` and the
completion audit.

Tests: DSP `stereo_enhancer_phase_offset_holds_one_side_back_by_exactly_its_time`,
`stereo_enhancer_width_balance_and_inversion_match_their_arithmetic`,
`stereo_enhancer_is_block_invariant_and_bypasses_without_a_click`; engine
`stereo_enhancer_phase_offset_and_inversion_reach_native_audio_history_and_files`; UI
`describe("Stereo Enhancer")` (three cases).

Least sure: `Controls<8>` destructuring and the borrow of `self.offset` beside `self.rate` in
the frame loop; the existing `spatial_reported_tails_really_expire` case with the new lines
(they are cleared when the tail expires).

Remaining gate: the phase offset glides when moved (10 ms control ramp), which bends pitch for
that moment; no pitch-preserving crossfade is claimed.

## `fx-fruity-panomatic` — Pan Motion

Implemented (`crates/windfall-dsp/src/control/motion.rs`): three appended controls, `shape`
(sine, triangle, saw, square, random; index 5), `sync` (6) and `beats` (7, quarter notes per
cycle while synced). Square, saw and random glide over their jumps with a 2 ms one-pole so a
pan never steps; sine and triangle are exact. The default (sine, free rate) keeps the previous
arithmetic. `set_tempo` now drives the rate while synced. The pan law stays inside the
processor (`control::balance`); no shared pan call site is touched. Editor
`motion/pan-motion-editor.tsx` with a two-cycle picture of pan and volume.

Tests: DSP `pan_motion_draws_each_shape_at_its_rate`, `pan_motion_follows_the_tempo_when_synced`,
`pan_motion_pan_volume_and_gain_process_the_audio`,
`pan_motion_is_block_invariant_and_bypasses_without_a_click`; engine
`pan_motion_shape_sync_and_volume_reach_native_audio_history_and_files`; UI
`describe("Pan Motion")` (three cases); helpers `the pan motion`.

Least sure: the existing `control/tests.rs` cases `pan_motion_modulates_audio_and_volume` and
`invariant::<PanLfo>` after the rewrite of `tick`; whether `ParamControl` with
`choiceStyle="select"` puts `data-param` where the UI helper `choose` looks for it.

Remaining gate: the motion free-runs from the start of processing; it is not locked to the
song position (the `Effect` interface carries tempo, not transport position).

## `fx-equo` — Morph EQ

Implemented (`crates/windfall-dsp/src/eqbank/graphic.rs`, `morph.rs`): the existing kind keeps
its 25 descriptors (two snapshots of four movable bells and `morph`) and gains a real graphic
equalizer: `curves` — four stored curves, A to D, of fifteen fixed bands two thirds of an
octave apart (25 Hz to 16 kHz, Q 2.1, -18 to 18 dB; indices 25 to 84) — and `curveCount`
(index 85; 2 to 4, default 2). The one `morph` control walks the curves in use, evenly spaced
on its travel, in straight lines of dB, and at the same time glides the bells from snapshot A
to B. A position on a stored curve gives that curve exactly. With flat curves the output is
bit-identical to before, so saved projects sound the same. `MorphEqParams::interpolated_curve`
exposes the gains at the current position. Descriptor count goes from 25 to 86
(`eqbank/tests.rs` updated).

Editor `features/effects/graphic/morph-eq-editor.tsx`: the selected stored curve on fifteen
faders, curve buttons A to D (only those in use), Morph on a horizontal fader with its
position in words, "Store what is heard in X" and "Flat" as single undo steps, the stored
curves drawn faintly behind what is heard, and the movable bells behind a switch.

Tests: DSP `morph_eq_curve_is_fifteen_fixed_bells_in_level_and_phase`,
`morph_eq_morph_walks_the_stored_curves_in_straight_lines_of_decibels`,
`morph_eq_is_block_invariant_through_a_morph_and_bypasses_without_a_click`; engine
`morph_eq_curves_and_morph_reach_native_audio_history_files_and_automation`; UI
`describe("Morph EQ")` (five cases); helpers `the Morph EQ curve`.

Least sure: the 61 generated `param_set!` rows compile as written (array-of-struct paths such as
`curves[2].gains_db[7]`); `EffectParams` grows by 244 bytes; the mounted cases need regenerated
`descriptors.json` and bindings (`MorphCurveParams` is a new exported type).

Remaining gate: no per-band pan or send, and no analysis-driven curve capture; "Store what is
heard" captures the morphed setting, not the audio.

## 2026-10-11 continuation - merge and Band Split

Merged `feat/parity-100` at `07753e49`, retaining all stereo/graphic editors and
Tuner. Generated artifacts in that merge came from integration; no generated
files were authored by this worker.

Band Split now declares three splitting outputs in `EffectKind::aux_outputs`;
`AnyEffect::process_aux` invokes its real `process_routed` seam. The existing
`Mixer.effectOutputs` / `SetEffectOutput` persistence, checked graph admission,
route pruning, PDC and rack splice all apply. `MAX_AUX_OUTPUTS` becomes three;
the engine allocates three fixed scratch buffers during construction. Both
slot and engine buffer borrowing now include the third output. No second
routing mechanism or new command. Two-band mode keeps Mid silent and preserves
its saved route for switching back to three bands.

Purpose-built `band-split/band-split-editor.tsx` shows actual crossover curves,
mode-dependent controls and per-band levels; the existing inspector supplies
Low/Mid/High destination selectors.

Additional tests (written, not executed):
- DSP `band_split_recombination_matches_independent_allpass_level_and_phase`;
  earlier separate-band measurement, partition and bypass tests remain.
- Engine `qa_band_split_routes_each_band_through_tracks_history_files_and_export`
  in `mixer_effect_outputs_qa.rs`: both modes, single-output isolation,
  recombination, downstream polarity, source fader/chain independence, bypass,
  invalid output refusal, undo/redo, save/reopen, live block sizes and export.
- Project `send_tap_transient_split_and_band_split_have_routable_outputs`.
- Mounted `describe("Band Split")` in `eq-filter-stereo.test.tsx` and
  `routes Low, Mid and High through the shared inspector with history and persistence`
  in `effect-outputs.qa.test.tsx`.

Integrator: run the existing commands above plus
`cargo test -p windfall-engine --test mixer_effect_outputs_qa`,
`cargo test -p windfall-project --test effect_outputs`, and after generation
`pnpm --dir apps/desktop test src/features/mixer/effect-outputs.qa.test.tsx --maxWorkers=4`.
Regenerate descriptors (`effectOutputs.bandSplit`), simulator and ordinary
bindings. No new type/command for Band Split. Check third-buffer borrowing,
expanded engine route capacity, test fixture render length and exact float
comparisons first. All validation remains unexecuted as required.

## Resonant sweep and seven-band parametric EQ - source reuse

The exact summaries do not require separate persisted alias kinds. The existing
Selectable filter is a real resonant SVF with audio-rate-safe smoothing and
host automation. A new `filter/filter-editor.tsx` exposes its mode, frequency,
resonance and gain beside the actual transfer, with existing parameter context
menus for drawing automation. This maps `fx-fruity-filter` to `selectableFilter`
without creating another copy of the Free Filter processor. It supplies actual
sweeps, not a neutral preset. Existing descriptor indices and saved settings stay
unchanged.

The baseline `eq` is seven independently movable bands: low cut, low shelf,
three bells, high shelf, high cut. Its existing purpose-built node editor is the
freely placed interface distinct from Seven Band's fixed faders and Morph EQ's
stored graphic curves. This satisfies the supplied `fx-fruity-parametric-eq`
summary (seven-band parametric equalizer); it does not claim seven arbitrary
switchable band shapes or proprietary DSP. No alias or duplicate editor is
added. Live spectrum comes from the merged analyzer work; the legacy row's
summary does not require spectrum acceptance (EQ2 retains that wider gate).

Tests added (written, not executed): DSP
`resonant_sweep_filter_matches_independent_frequency_phase_and_level_response`,
`resonant_sweep_is_block_invariant_and_bypasses_continuously`,
`seven_movable_eq_bands_match_an_independent_complex_response`,
`movable_eq_is_block_invariant_and_bypasses_continuously` in `eq_filter_stereo.rs`.
Engine `resonant_sweep_reaches_native_audio_history_files_and_automation` and
`seven_movable_eq_bands_reach_native_audio_history_files_and_automation` in the
existing stream acceptance target. Both kinds join the project's all-control,
legacy/default, history/disk and automation tests. Mounted `describe("Resonant
filter")` and `describe("Seven movable EQ bands")` exercise the real WASM document.
Existing `editors.test.tsx` has the richer EQ pointer/Q/selection gestures.

Integrator: existing stream commands cover these additions; also run
`pnpm --dir apps/desktop test src/features/effects/editors.test.tsx src/features/effects/filter-family.test.tsx --maxWorkers=4`.
No new generated types. Regenerate simulator for the complete stream. Check
measured shelf-phase tolerances and live/export exactness first. Existing EQ
allocator and SVF reference/automation suites remain applicable. Source reuse
is an explicit mapping decision for the overseer's row acceptance.

## Eight-unit drawn Filter Bank - 2026-10-11

Implemented `eqbank/pattern.rs`, exposed as `FilterPatternParams` and
`FILTER_PATTERN_POINTS`. Each unit has an enabled toggle, 0.25-32 quarter-note
cycle length, 0-8 octave cutoff depth and eight fixed, equally spaced points
for each of gate (0-1) and cutoff (-1 to 1). Cycles wrap continuously to point 1.
Base cutoff times `2^(point * depth)` is clamped to 20-20000 Hz and Nyquist.
Parameter writes prepare eight coefficient endpoints; audio only interpolates
endpoints and smooths continuously moving targets over a nominal 5 ms.
Interpolation is in coefficient space, not logarithmic Hz between points.
Volume gates use the same continuously retargeted 5 ms ramp. Static edits land
in 5 ms; continuously moving curves have smoothing lag. No sample-path
transcendentals, allocations, frees, IO or locks.

Serial applies each filter and gate to the preceding unit's audio. Parallel
feeds the source to independent filter/gate histories, averages active branches
and crossfades routing without resetting histories. The existing first 25
indices are unchanged; pattern controls occupy 25-176. `FilterSlotMode::Dry`
is appended to each mode's choice list (index 4); it is an active unfiltered
branch, while Bypass skips/excludes a unit. Bypassed units ignore their patterns.
Tail includes the worst stored cutoff endpoint; gap reserves that memory so a
closed gate cannot cause premature retirement before reopening.

`FilterBankParams.patterns` has a legacy default and is omitted when entirely
default. Desktop `params/default-fields.ts` materializes that one known omitted
field for read/optimistic editing, via two narrow calls in `params/access.ts`;
it does not mutate the snapshot or default data. New mounted editor
`filter-bank/filter-bank-editor.tsx` has eight unit tabs, real routing selection,
unit filters, tempo cycle/depth, SVG stroke drawing and accessible point faders.
Whole strokes and Open gate/Center cutoff use single history gestures.

Tests (written, not executed):
- DSP `filter_bank_real_series_and_parallel_routes_match_independent_complex_transfers`;
  `filter_bank_eight_gate_envelopes_follow_tempo_and_really_process_audio`;
  `filter_bank_drawn_cutoff_and_gate_patterns_are_block_invariant_and_bypass_continuously`;
  `filter_bank_patterns_default_out_of_legacy_json_and_descriptor_indices_stay_stable`;
  `drawn_cutoff_audio_matches_an_independent_time_varying_iir_reference`.
- Existing eqbank metadata/hostile/allocation cases now traverse 177 controls;
  default JSON metadata is explicitly materialized for its path assertions.
- Engine `eight_unit_drawn_filter_patterns_reach_native_audio_history_files_and_automation`.
- Existing project all-control/history/disk/default/automation cases cover all
  new controls. Mounted `describe("Filter Bank")` covers all eight units, point
  edits, a whole stroke, undo and redo; `default-fields.test.ts` covers omitted
  defaults without wrong-kind fallback or mutation.

Integrator commands: existing stream suites plus
`pnpm --dir apps/desktop test src/features/params/default-fields.test.ts --maxWorkers=4`.
Regenerate `FilterBankParams`, `FilterSlotMode`, new `FilterPatternParams`,
`EffectParams` dependency exports, descriptor table and simulator. No new
Command or IPC method. Check macro array paths, optional-field TS output,
expanded ParamSet snapshots and exact independent-reference tolerances first.

Limits: fixed eight-point repeating curves; no arbitrary-size envelope assets,
MIDI-triggered retrigger or song-position lock. The supplied summary requires
drawn gate/filter patterns and eight series/parallel units, which this exposes;
the cycle is processor-relative, following tempo, as with Pan Motion.
No executed evidence or proprietary sonic identity is claimed.

## Final implementation hand-off - 2026-10-11

All nine rows are full-row candidates against their supplied summaries, pending
integrator execution. No parity status file was edited. Prior worker work was
continued, and all integration additions in the requested merge were preserved.

Last acceptance additions (written, not executed):
- `band_split_registered_outputs_match_independent_band_level_and_phase` measures
  each registered aux output against independently derived LR4 complex responses
  at 44.1/48/96 kHz, in both band modes.
- `routed_band_split_mix_and_bypass_preserve_the_same_continuous_sum_as_the_plain_slot`
  compares routed+main to the plain slot through mix and bypass fades.
- The native three-output case counts callback allocations/frees after adoption.
- Filter Bank mounted cases select real Parallel/Dry routing, enable every unit's
  pattern, edit cycle/depth and draw points through the WASM history.
- Project `damaged_filter_patterns_are_refused_and_legacy_patterns_default`
  rejects out-of-range controls and wrong-length arrays on file load.

Shared production seams changed by this continuation (outside merge contents):
`crates/windfall-dsp/src/effect.rs` (third aux output and BandSplit admission),
`crates/windfall-engine/src/mixer.rs` (third prepared scratch buffer),
`apps/desktop/src/features/effects/effect-editor.tsx` (editor registration),
`apps/desktop/src/features/params/access.ts` (known omitted default field).
Other changes stay in stream-owned DSP/editor/test modules. No shared engine
pan call sites, project command union, IPC transport or session host were edited.
Existing checked effect-output routing owns validation/history/removal/PDC.

Generated inventory for the full stream: new `MorphCurveParams`, `StereoInvert`,
`FilterPatternParams`; changed `MorphEqParams`, `StereoEnhancerParams`,
`PanLfoParams`, `FilterBankParams`, `FilterSlotMode`, dependent `EffectParams`
exports, descriptors/automation fixtures and simulator. Regenerate once after
integration. No generated source or WASM was hand-authored. Integration's own
generated files were retained by the explicitly requested merge.

Risk order for the integrator:
1. Regenerate first: all mounted cases depend on fresh descriptors and WASM.
2. Compile native tests, checking nested-array macro paths and new optional
   pattern field export, and expanded aux buffer borrowing/capacity.
3. Independent DSP phase/timing tolerances and routed native sum/live/export
   exactness, especially two-band silent Mid and receiver processing.
4. Mounted eight-tab/choice/switch/stroke gestures and omitted-default access.
5. Run the existing all-kind hostile/allocator/engine realtime suites. The
   ignored Seven Band timing comparison remains the CPU measurement gate;
   no performance result has been fabricated.

Remaining contract boundaries: eight-point repeating Filter Bank envelopes and
Pan Motion start relative to processing, not song-position/MIDI retrigger;
Stereo Enhancer's moving phase delay temporarily bends pitch; Morph EQ stores
settings rather than analyzing/capturing audio. Those wider behaviors are not
part of the supplied summaries. Hardware/listening review is external. Every
new or previously authored test in this stream remains written, not executed.
