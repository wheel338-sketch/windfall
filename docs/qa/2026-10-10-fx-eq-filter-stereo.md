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
| `fx-spreader` | Spreader (`spreader`) | not started |
| `fx-fruity-stereo-enhancer` | Stereo Enhancer (`stereoEnhancer`) | not started |
| `fx-fruity-panomatic` | Pan Motion (`panLfo`) | not started |
| `fx-equo` | Morph EQ (`morphEq`) | not started |
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
