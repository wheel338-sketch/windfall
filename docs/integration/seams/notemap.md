# Bounded note transforms

Windfall's `notemap/` module contains note-event processors, with no audio processing
or `EffectKind` registration. The modular-rack stream registers `pub mod notemap` and integrates these
foundations through `dsp/rack_note.rs`, `project/rack.rs` and `engine/modular.rs`.
See [the rack host seam](modular-rack.md).

## Types and parity scope

`MappedNote` is Copy data containing `key: u8`, `velocity: f32`, `channel: u8`, and
`color: u8`. Keys clamp to MIDI 0..127, velocity to 0..1, and channel/color to 0..15.
Non-finite velocity becomes zero before the processor applies its controls.

All five processors are Copy + Default. Their corresponding `*Params` types are
Copy + Default, serde camelCase, and `ts_rs::TS`, with every control described and
addressed through `ParamSet` implemented by `param_set!`. RackNoteModule references now export these TS dependencies during the normal
binding generator pass; fixed Rust tables have TS array views for editor copies. Parameters are sanitized in
`set_params`; changes apply immediately without smoothing.

| Processor / parameters | Parity ID | Behavior and limits |
| --- | --- | --- |
| `ColorMap` / `ColorMapParams` | `fx-vfx-color-mapper` | Fixed 16-entry identity-default table. Changes color only; channel remains independent. |
| `LevelScale` / `LevelScaleParams` | `fx-vfx-level-scaler` | Velocity times percent/100, then floor/ceiling. Percent 0..200; bounds 0..1. Reversed bounds raise ceiling to floor. Velocity only: this event has no pan or expression fields. A positive floor can raise sanitized zero velocity. |
| `KeyMap` / `KeyMapParams` | `fx-vfx-key-mapper` | Copy `[u8; 128]` identity-default table, output keys clamped to 0..127. Collisions preserve separate events. JSON tables require exactly 128 entries; omitted table uses identity. |
| `KeySplit` / `KeySplitParams` | `fx-vfx-keyboard-splitter` | Low range `[lowMin, boundary)`, high range `[boundary, highMax]`. Other keys drop. Reversed ranges are empty. Each accepted note takes the range's fixed destination key/channel, retaining color. Defaults: boundary 60, full input range, low destination 48/channel 0, high destination 72/channel 1. |
| `StepGrid` / `StepGridParams`, `GridStep` | `fx-vfx-sequencer` | **Partial step transform.** Sixteen gates, pitch offsets -127..127, velocity multipliers 0..2; pitch/velocity clamp after shifting/scaling. Default gates open with neutral pitch/velocity. Does not generate notes, schedule releases, or implement the channel arpeggiator. |

## Host integration

Import `NoteTransform` to call `set_params`, `map`, and
`transform<const N: usize>(&mut [MappedNote; N], len) -> usize`. `map` produces
zero or one output. `transform` processes `min(len, N)` inputs and compacts accepted
notes into the same array in stable order. Only the returned prefix is valid;
the unused tail has no defined contents. Chaining transforms feeds each returned
length into the next. Empty arrays and lengths above capacity are safe.

Transforming, updating parameters, copying/dropping processors, and advancing the
step clock allocate nothing, free nothing, take no locks, and perform no IO. Work
is bounded by the caller's array capacity; no processor expands the event count.
Serde and TS rendering are host-side setup operations outside that guarantee.

The host keeps pan and `NoteExpression` separately and passes them to
`Instrument::note_on_expression` or `note_on_instance`. Map color to
`NoteExpression::color_group` only when that is the host's chosen color policy;
use the independent channel for host routing. Preserve the existing `None` color
policy separately: `MappedNote` cannot represent it. Articulation, glide duration,
fine pitch, modulation, and release amounts are not changed here.

The host must retain each accepted note's mapped key/channel with its
`NoteInstanceId`, so note-off reaches the original output even after a parameter
or grid-step change. Key collisions require separate instance tracking; do not
merge them. Dropped note-ons must not release unrelated voices. These transforms
process note-on candidates only: do not run note-offs through current gates/maps.
Handle zero output velocity as silence according to host policy; the Instrument
API treats non-positive note-on velocity as a note-off.

`StepGrid` starts at step zero, 120 BPM, four steps per quarter-note beat.
`set_tempo` clamps finite BPM to 1..1000 and restores 120 for non-finite BPM.
`advance_samples(samples: u64, sample_rate: f32)` advances fractional step phase,
wrapping every 16 steps. Invalid or non-positive sample rates do nothing. Tempo
and parameter updates retain phase; `reset` returns to step zero. Advance up to
each event's sample position before transforming it; a whole batch sees one step.
At 120 BPM and 48 kHz, the default step lasts 6000 samples (125 ms).

The clock uses f64 phase arithmetic; very large sample jumps lose fractional
precision. For transport seeks, reset and advance to the desired position. This
seam has no transport ownership, timestamp, duration, delayed queue, MIDI output,
UI, project persistence wiring, or registry integration.

## Validation

The declaration is now permanent; no temporary source changes are needed.
From Git Bash (`C:\Program Files\Git\bin\bash.exe`) at the workspace root:

```bash
source scripts/msvc-env.sh
cargo test -p windfall-dsp --lib notemap
```

Keep the permanent declaration: modular rack note processors depend on it.
Unit tests under `notemap/tests.rs` cover color tables, velocity bounds,
identity/colliding remaps, split boundary/drop compaction, tempo/gate/pitch/velocity,
non-finite input, parameter descriptors, JSON tables, and TS declarations.

The library's existing unit-test allocator prevents installing a second global
allocator there. The independent probe under `notemap/verify.rs` imports this
module's actual source and the crate's actual parameter macro and math helpers:

```bash
source scripts/msvc-env.sh
bash crates/windfall-dsp/src/notemap/verify.sh
```

It checks zero allocations, reallocations, and frees during all five fixed-array
transforms and parameter updates, plus step-clock operations. A positive control
verifies the probe observes heap activity. It also runs the module's unit tests.
This standalone probe works with the temporary declaration removed.
