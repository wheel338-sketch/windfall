# Delay, modulation and reverb effects (roadmap family E3)

Branch `worktree-agent-ad7cbd74d410112df`, from `feat/parity-100` at `fd60cdde`.

**Nothing in this report was compiled or run.** The work was done under an
edit-only rule: no Cargo, no pnpm, no generators. Every test named below is
"written, not executed". The parent builds, regenerates and runs.

This file is kept current as the work goes, so it is a usable hand-off if
the session stops early. The status column says how far each row got.

## Resumed implementation, 2026-10-11

Merge `7ae5c5fc` retains both delay editor registrations and the integration
branch's tuner registration, and retains both engine module additions.
The incoming generated bindings/WASM are integration artifacts, not worker
edits. No worker generator, build, test, or browser validation was run.

Source review after the merge confirmed that `EffectSlot::try_prepare` still
matches the slot preparation buffers, `EffectUnit::build` has one caller, and
the control-side `PlanState::try_build` performs admission before allocation.
The linked editor, drawn editor, admission and saturating tails are therefore
full-row **candidates** for the two delay-bank rows, pending integrator execution
of the commands below. The earlier checkpoint also appended bucket modes,
chorus voice count, phaser stage/stagger controls and flanger order, but left
their declared `spatial::model_tests` module absent; this resumption completes it.

## Status by row

| Row | Status | What is in the branch |
| --- | --- | --- |
| `fx-fruity-delay` | complete in source (`61cd12b6` and the test commit); nothing run | Inverted stereo mode in the core Delay, with DSP, importer, engine and UI tests |
| `fx-fruity-delay-bank` | all four named gates addressed in source (`7ecea4c7`, `ef0ce026`, editor commit); nothing run | Linked eight-unit editor, fallible host admission, saturating tails |
| `fx-multiband-delay` | all four named gates addressed in source; nothing run | Drawn sixteen-band delay/level/pan editor, same host gates |
| `fx-luxeverb` | full-row candidate; nothing executed | Sized, pitch-shifted and modulated spaces with dedicated editor |
| `fx-fruity-reeverb` | full-row candidate; nothing executed | Existing room, reflection readout and grouped room/decay editor |
| `fx-vintage-chorus` | DSP/test candidate; editor integration in progress | Clocked bucket modes and free voices, tempo sync |
| `fx-vintage-phaser` | DSP/test candidate; editor integration in progress | Four-stage equal-corner pedal circuit, stage/stagger controls, tempo sync |
| `fx-hyper-chorus` | DSP/test candidate; editor integration in progress | Two to sixteen actual detuned voices per side, tempo sync |
| `fx-fruity-flangus` | DSP/test candidate; editor integration in progress | One to four serial swept combs, correct four-comb warmup, tempo sync |

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
- `crates/windfall-engine/tests/delay_inverted_qa.rs`:
  `inverted_delay_swaps_the_echo_sides_persists_and_matches_live_offline`
  (a hard-left source, echoes only: stereo mode leaves them left, inverted
  puts the same energy on the right; undo/redo, save/load, offline against
  live at two block sizes).
- `apps/desktop/src/features/effects/delay-family.test.tsx`:
  "the delay's inverted stereo mode" (third segment of the mode control,
  `setEffectParam` with index 4 and value 2, undo/redo, save/reopen). It
  needs regenerated descriptors to find the third segment.

Commands:

```bash
cargo test -p windfall-dsp --lib delay::tests
cargo test -p windfall-flp --lib the_older_delay
cargo test -p windfall-engine --test delay_inverted_qa
pnpm --dir apps/desktop test src/features/effects/delay-family.test.tsx --maxWorkers=4
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

## 2026-10-11 circuit continuation

[E3-CIRCUITS.md](../E3-CIRCUITS.md) gives the public behavioural references,
authored circuit equations/limits, appended indices and exact row mapping.
No dependencies, assets, commands, IPC variants or EffectKind variants added.
The old generic hosting path already carries the six registered kinds through
document commands, engine construction and realtime/offline processing.

The previously missing `spatial/model_tests.rs` is now supplied. Continuous and
topology controls glide per frame. Chorus voice weights/phaser stage weights/
stack order weights retain first-frame and legacy defaults. Four stacked combs
now report 64 ms warmup instead of the previous two-comb 32 ms. Lush reserves
its largest size and pitch histories in preparation; its wet pitch is purely
feed-forward and adds no feedback cycle. Zero amount keeps plain hall audio.

### Per-row tests (all written, not executed)

| Row | Direct DSP measurement | Native PCM/history/file case |
| --- | --- | --- |
| `fx-fruity-reeverb` | full-row candidate; nothing executed | Existing room, reflection readout and grouped room/decay editor |
| `fx-luxeverb` | full-row candidate; nothing executed | Sized, pitch-shifted and modulated spaces with dedicated editor |
| `fx-vintage-chorus` | `bucket_clock_has_128_samples_in_transit_and_a_bounded_work_limit`, `bucket_sweeps_have_opposed_triangles_and_the_both_mode_stays_mono`, `bucket_modes_ignore_free_controls_and_have_distinct_audio` | `bucket_mode_changes_native_character_and_roundtrips` |
| `fx-vintage-phaser` | `four_equal_allpasses_produce_two_measured_pedal_notches` | `pedal_stage_configuration_changes_native_notches_and_roundtrips` |
| `fx-hyper-chorus` | `swarm_count_measures_the_average_of_independently_delayed_voices` | `sixteen_voice_chorus_changes_native_detuning_and_roundtrips` |
| `fx-fruity-flangus` | `stacked_combs_match_an_independent_difference_equation_for_every_order` | `four_stacked_combs_change_native_audio_and_roundtrip` |

All modulation direct cases are in `crates/windfall-dsp/src/spatial/model_tests.rs`.
Its shared cases `model_extrema_partition_edits_empty_blocks_and_tails_are_stable`,
`tempo_sync_matches_free_rate_and_depth_zero_removes_detuning` and
`legacy_model_settings_keep_defaults_and_descriptor_prefixes` cover each circuit.
The six named native cases are in `crates/windfall-engine/tests/e3_circuits_qa.rs`;
each measures real changed PCM, gesture undo/redo, disk save/load and exact live
versus offline audio at 7/64/127-frame partitions.

All six also have:

- `crates/windfall-dsp/tests/dsp/e3_circuits.rs`, registered in `dsp/main.rs`:
  `e3_slots_allocate_nothing_through_topology_tempo_reset_bypass_and_tail_expiry`,
  `e3_bypass_wake_and_parameter_edits_are_partition_invariant_and_return_exact_dry`.
- `crates/windfall-project/tests/e3_circuits.rs`: every control via real checked
  commands; `settings_gestures_noops_and_disk_save_reopen_preserve_every_control`,
  `v1_defaults_roundtrip_without_a_format_bump`,
  `invalid_edits_are_atomic_and_finite_extrema_use_descriptor_sanitization`,
  `every_control_has_the_correct_automation_range_and_removal_history_restores_curves`.
- `apps/desktop/src/features/effects/e3-circuits.test.tsx`: six mounted descriptor/
  distinct-control/history/file cases, six mounted automation cases, bucket model
  disabling/return-to-Free, and four mounted tempo-cycle switches. Requires new
  descriptors and Rust WASM; using the old generated artifacts does not validate it.

### Integrator commands

Use Git Bash, `source scripts/msvc-env.sh`, and a task-specific temporary
`TS_RS_EXPORT_DIR` as in roadmap section 3. Regenerate bindings/descriptors and
WASM on integration, then run the prior delay commands and:

```bash
cargo test -p windfall-dsp --lib spatial
cargo test -p windfall-dsp --lib lush
cargo test -p windfall-dsp --test dsp e3_circuits
cargo test -p windfall-project --test e3_circuits
cargo test -p windfall-engine --test e3_circuits_qa
pnpm --dir apps/desktop test src/features/effects/e3-circuits.test.tsx src/features/effects/delay-family.test.tsx --maxWorkers=4
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```

The worker ran only rustfmt on owned Rust files; no compilation, test, generator
or browser run. The existing lush allocator probe test compiles a standalone
probe internally; that is integrator work, not a worker execution.

### Regeneration and risks to check in order

1. Regenerate `VintageChorusParams`, `HyperChorusParams`, `VintagePhaserParams`,
   `StackedFlangerParams`, `BucketMode`, `PhaserStages`, `LushSpaceParams`, their
   effect descriptors, umbrella exports and simulator dependency tree. `DelayMode`
   still needs its earlier inverted variant. No new command or IPC shapes.
2. Compile owned Rust and native fixtures first. The previously absent model test
   module is now present. Verify independent phaser/comb/chorus tolerance bounds
   before adjusting them; none has empirical pass evidence from this worker.
3. Check Lush pitch projection at +/-12 semitones, size-dependent arrival times,
   the enlarged lifetime, and legacy-default impulse tests. Pitch voice is a
   windowed delay pitch shifter, not a formant-preserving vocal engine.
4. Run mounted UI with regenerated descriptors/WASM. New panel sizes and disabled
   controls have source review only. Frontend formatting/lint may need integrator
   adjustment; the worker was prohibited from invoking frontend tooling.
5. Run native live/offline and history cases, then allocator/bypass regressions.
   Bank auto-tail remains conservative (roughly 200 s gap) and the 1 GiB admission
   limit covers admitted echo histories, not every host allocation or old plan.

All these rows remain candidates until the integrator passes the authored suites.
This is summary-behaviour parity, not a claim of every reference plugin's extra
control, proprietary preset, hardware noise or sound-identical emulation.
