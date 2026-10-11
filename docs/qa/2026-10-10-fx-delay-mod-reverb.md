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
| `fx-fruity-delay-bank` | pending | |
| `fx-multiband-delay` | pending | |
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

## Regeneration the parent must run

- `scripts/gen-bindings.sh` and `scripts/build-sim.sh`: `DelayMode` gains
  a variant and the Delay descriptor's mode choice gains an option.

## Shared seam files touched

None so far.
