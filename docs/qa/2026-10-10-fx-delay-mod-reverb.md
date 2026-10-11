# Delay, modulation and reverb effects (roadmap family E3)

Branch `worktree-agent-ad7cbd74d410112df`, from `feat/parity-100` at `fd60cdde`.

**Nothing in this report was compiled or run.** The work was done under an
edit-only rule: no Cargo, no pnpm, no generators. Every test named below is
"written, not executed". The parent builds, regenerates and runs.

This file is kept current as the work goes, so it is a usable hand-off if
the session stops early. The status column says how far each row got.

## Status by row

| Row | Status | What is in the branch |
| --- | --- | --- |
| `fx-fruity-delay` | DSP and importer committed (`61cd12b6`); engine and UI tests pending | Inverted stereo mode in the core Delay |
| `fx-fruity-delay-bank` | all four named gates addressed in source (`7ecea4c7`, `ef0ce026`, editor commit); nothing run | Linked eight-unit editor, fallible host admission, saturating tails |
| `fx-multiband-delay` | all four named gates addressed in source; nothing run | Drawn sixteen-band delay/level/pan editor, same host gates |
| `fx-luxeverb` | pending | |
| `fx-fruity-reeverb` | pending | |
| `fx-vintage-chorus` | pending | |
| `fx-vintage-phaser` | pending | |
| `fx-hyper-chorus` | pending | |
| `fx-fruity-flangus` | pending | |

## `fx-fruity-delay`: basic echo with ping-pong, inverted stereo and a lowpass

Public behaviour read 2026-10-10 from the
[Fruity Delay manual page](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/plugins/Fruity%20Delay.htm):
"Inv Stereo swaps the right and left channels for all echoes", "Ping Pong
swaps the left and right channels of the input for each echo", and a cutoff
that lowpasses the echoes. The page gives no sample equation; the one below
is Windfall's.

Implemented in `crates/windfall-dsp/src/delay.rs`:

- `DelayMode::Inverted`, appended after `Stereo` and `PingPong`. The mode
  control keeps descriptor index 4 and its stored values 0 and 1; its
  maximum goes from 1 to 2. Stored JSON is `"inverted"`.
- Each side's input goes into the other side's delay line. Feedback stays
  in that line, so a sound on the left echoes on the right and every
  repeat stays on the right. The dry sound keeps its place.
- A second 20 ms ramp (`swap`) glides between the routings. While it rests
  at zero the stereo and ping-pong arithmetic is the code that was there
  before, sample for sample.
- The existing High cut is the row's lowpass. Tail and gap are unchanged.

`crates/windfall-flp/src/convert/effects.rs`: the older FL delay's mode 1
now imports as `DelayMode::Inverted`. Fruity Delay 2 still reports its
inverted mode as not carried over, because its existing test asserts that
note and its manual wording was not checked.

Tests written, not executed:

- `crates/windfall-dsp/src/delay.rs`, module `tests`:
  `the_inverted_mode_is_appended_after_the_existing_choices`,
  `inverted_echoes_arrive_on_the_other_side_at_the_delay_time`,
  `inverted_repeats_stay_on_the_swapped_side_and_decay`,
  `stereo_and_ping_pong_still_keep_their_sides`,
  `switching_to_inverted_glides_and_does_not_depend_on_the_blocks`,
  `inverted_keeps_the_stereo_offset_and_the_tail_of_the_other_modes`.
- `crates/windfall-flp/src/convert/effects.rs`:
  `the_older_delay_keeps_its_inverted_stereo_mode`.

Commands:

```bash
cargo test -p windfall-dsp --lib delay::tests
cargo test -p windfall-flp --lib the_older_delay
```

Least sure:

- `inverted_repeats_stay_on_the_swapped_side_and_decay` bounds the energy
  ratio between repeats to 0.12–0.2501. The upper bound is exact (feedback
  0.5 squared, filters only attenuate). The lower bound assumes the 20 kHz
  high cut and 20 Hz low cut take less than half the energy of an impulse
  per pass at 44.1 kHz. Loosen the 0.12 if it fails.
- The frontend's `DelayMode` binding gains `"inverted"` on regeneration.
  The delay editor draws the mode from the descriptor, so it shows a third
  segment without a source change.

## `fx-fruity-delay-bank` and `fx-multiband-delay`: the four gates

`docs/DELAY-FAMILY.md` and `docs/qa/2026-10-09-delay-bank.md` held both rows
open on four things. Each is now in source. The DSP of `EchoBank` and
`FrequencyDelay` was not changed.

### Saturating aggregate tails

Both processors report `usize::MAX` as their tail while any feedback is up.
`PlanState::settled` in `crates/windfall-engine/src/state.rs` added tails
with `sum::<usize>()` and then added the result to a `u64` frame, which
overflows (a panic in a debug build, a wrapped short tail in release) as
soon as one such effect shares a track with any other tail.

- New `crates/windfall-engine/src/effect_host.rs`: `total` (saturating sum
  of tails) and `horizon` (saturating frame arithmetic).
- `state.rs` `settled` uses them for the ring, the gap and both horizons.
  A track with an endless tail is never "rung out" by counting; it is done
  only when its input and output have been quiet for the summed gap.

Known limit, not changed: `EchoBank::gap_samples` is its whole conservative
horizon, eight units of 25 seconds, about 200 seconds. An automatic-tail
render through an echo bank therefore always runs the full `tail_secs` the
user allows. It ends correctly at that horizon; it does not end early. A
tighter gap (the longest enabled path) would be a DSP change with its own
proof and was left alone.

### Fallible host preparation admission

- New `crates/windfall-dsp/src/effect/admission.rs` (a child module, so
  `effect.rs` gains one line, `mod admission;`):
  `EffectKind::history_bytes`, `AnyEffect::try_prepare`,
  `AnyEffect::preparation_status`, `AnyEffect::preparation_refusal`,
  `EffectSlot::try_prepare`. Only the two delay kinds can be refused; every
  other kind ignores the budget.
- `effect_host.rs` `HistoryAdmission::of(plan, rate)` reserves the history
  of every non-leaving effect of the plan against `ECHO_HISTORY_LIMIT`
  (1 GiB: thirteen banks at 48 kHz, one at 384 kHz) before anything is
  prepared, and gives each new effect its own share as its budget.
- `EffectUnit::build` in `rack.rs` takes that budget, calls
  `EffectSlot::try_prepare` and returns the refusal. Its one caller,
  `PlanState::try_build`, turns the first refusal into
  `ProjectPreparationError::MemoryLimit` (budget), `SizeOverflow`, or
  `Unsupported` with a sentence (the allocator said no), and returns it.
  Everything built so far is dropped there, on the control side, and the
  plan that was playing stays.
- `project_preparation.rs`: `MemoryLimit` now displays as a sentence with
  megabytes. The desktop session already shows preparation errors as project
  warnings, so the refusal reaches the user.

No rate is clamped and no delay shortened to fit. Leaving effects are not
charged: their memory is held already and is on its way back.

### Linked eight-unit editor

`apps/desktop/src/features/effects/echo-bank/` (`echo-bank-editor.tsx`,
`chain.ts`), registered for `echoBank` in `effect-editor.tsx`.

- A chain strip shows all eight units at once: on/off light, a tile with
  the unit's time and whether it takes the bank's input and is heard, and
  between each pair the knob of the send to the next unit with an arrow
  that lights where sound really passes (both units on, send not zero).
- A readout puts the routing in words: `1 → 2 → 3, 4`.
- **Series** and **Parallel** rewire every unit in one undo step.
- Picking a tile shows every setting of that unit in five groups: time,
  in and out, repeats, filter, repeat filter. Which unit is shown is view
  state and is not saved or sent.
- Every control is a bound `ParamControl`, so each setting keeps its
  automation menu, gesture undo and descriptor index.

### Drawn sixteen-band editor

`apps/desktop/src/features/effects/frequency-delay/`
(`frequency-delay-editor.tsx`, `band-plot.tsx`, `bands.ts`), registered for
`frequencyDelay`.

- Sixteen bars, one lane at a time: Delay, Level or Pan. Press and drag
  across to draw; bands a fast stroke jumps over are filled in on the
  straight line; a stroke that leaves the plot keeps drawing along its
  edge. One stroke is one undo step.
- Each bar is a slider of its own: arrows, Page Up/Down, Home/End, Shift
  for fine steps, left/right to walk the bands, double-click to reset, and
  the automation menu of its setting.
- Under each bar: the band's nominal center frequency and its on/off light.
  A band that is off is drawn dim.
- A readout gives the longest echo that will be heard under Scale and
  Short range.

### Tests written, not executed

- `crates/windfall-dsp/src/effect/admission.rs`, module `tests`:
  `only_the_two_large_delay_kinds_ask_for_admission`,
  `a_slot_prepares_within_exactly_what_its_kind_asks_for`,
  `one_byte_too_few_is_refused_and_named`,
  `a_refused_replacement_leaves_a_prepared_slot_sounding_as_it_did`,
  `every_other_kind_ignores_the_budget_and_matches_plain_preparation`.
- `crates/windfall-engine/src/effect_host.rs`, module `tests`:
  `tails_without_end_add_up_to_without_end`,
  `a_horizon_never_wraps_round_to_an_early_frame`,
  `a_plan_is_admitted_up_to_the_limit_and_refused_past_it`,
  `each_new_effect_is_prepared_within_its_own_share`,
  `a_refused_preparation_becomes_a_project_refusal`.
- `crates/windfall-engine/tests/delay_host_gates_qa.rs`:
  `two_endless_tails_on_one_track_end_at_the_chosen_horizon`,
  `endless_tails_that_have_gone_quiet_are_asked_about_without_overflow`,
  `a_plan_past_the_echo_history_limit_is_refused_and_the_old_one_plays_on`.
- `apps/desktop/src/features/effects/delay-family.test.tsx`: 3 routing
  cases, 4 plot-geometry cases, 7 echo bank editor cases, 8 frequency delay
  editor cases, mounted against the mock backend's shared Rust document.

Commands:

```bash
cargo test -p windfall-dsp --lib effect::admission
cargo test -p windfall-engine --lib effect_host
cargo test -p windfall-engine --test delay_host_gates_qa
cargo test -p windfall-engine --test delay_bank_acceptance_qa
pnpm --dir apps/desktop test src/features/effects/delay-family.test.tsx --maxWorkers=4
```

Least sure:

- `EffectSlot::try_prepare` copies the body of `EffectSlot::prepare` after
  the effect's own preparation. If another stream changed `prepare` (new
  scratch buffers, say), mirror the change in `effect/admission.rs`.
- `EffectUnit::build` changed signature (a budget in, a `Result` out). It
  had one caller. If a merged stream added another, it needs
  `PreparationBudget::UNLIMITED` and an `.expect` or a `?`.
- `a_plan_past_the_echo_history_limit...` relies on `Controller::set_project`
  latching a `PlanState::try_build` error in `project_preparation_error()`.
  If the convenience path builds state elsewhere, assert on that path's
  error instead. If the admission were broken this test would try to
  allocate about 1 GB; it should refuse before allocating anything.
- `one_byte_too_few_is_refused_and_named` assumes a first refusal leaves
  `is_prepared` false, as `DELAY-FAMILY.md` states.
- The UI tests assume the mock backend stores settings as 32-bit floats
  (the shared Rust document does) and that one gesture across several
  settings is one history entry, as the equaliser's drag test shows.
- Tailwind classes in the two editors were chosen from tokens used
  elsewhere (`bg-display`, `bg-brand`, `bg-chassis`, `font-readout`); the
  layout was never seen rendered. Expect to adjust sizes by eye.

## Regeneration the parent must run

- `scripts/gen-bindings.sh` and `scripts/build-sim.sh`: `DelayMode` gains
  a variant and the Delay descriptor's mode choice gains an option.

## Shared seam files touched

- `crates/windfall-dsp/src/effect.rs`: one line, `mod admission;`, before
  the registry tests. No registry, enum or macro arm changed.
- `apps/desktop/src/features/effects/effect-editor.tsx`: two imports and two
  entries appended to `EDITORS`.

Engine files that are not on the seam list but that other streams may also
touch: `crates/windfall-engine/src/lib.rs` (one `mod` line at the end),
`rack.rs` (`EffectUnit::build` signature), `state.rs` (admission before
plugin preparation, the fallible build of each effect, saturating sums in
`settled`), `project_preparation.rs` (one `Display` arm).
