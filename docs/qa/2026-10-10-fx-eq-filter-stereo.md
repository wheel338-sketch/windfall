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
| `fx-fruity-love-philter` | Filter Bank (`filterBank`) | not started |
| `fx-fruity-filter` | new kind | not started |
| `fx-fruity-parametric-eq` | new kind | not started |
| `fx-frequency-splitter` | Band Split (`bandSplit`) | not started |

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
