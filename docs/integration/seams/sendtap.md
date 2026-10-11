# Send Tap DSP seam

`SendTap` / `SendTapParams` live in `crates/windfall-dsp/src/sendtap/`.
The display name is **Send Tap**. Target parity ID: `fx-fruity-send`, **partial
parity: a levelled copy at this point in the effect chain**.

`SendTapParams` is `Copy`, `Default`, serde camelCase, `ts_rs::TS`, and
implements `ParamSet` through `param_set!`. Its only control is `level`, a
linear gain from 0 to 1, default 0. Nonfinite levels sanitize to 0; finite
values clamp into range. No automatic TypeScript export test is generated.

`process()` leaves both dry slices bit-for-bit unchanged at every level,
including signed zero, subnormals, and nonfinite audio. No block is captured
internally. After `process()`, the host calls:

```rust,ignore
tap.process(&mut left, &mut right);
tap.send(&left, &right, &mut send_left, &mut send_right);
```

All four `send()` slices must have equal lengths. The host owns the source
block and destination stereo pair, and must call `send()` before downstream
effects change the source or parameter updates change the level. `send()`
overwrites the destination; it does not accumulate. Level 0 writes exact
silence even for nonfinite source audio. Level 1 copies the source bit-for-bit;
intermediate levels multiply each sample by the gain. Level changes apply
immediately without smoothing. Reset retains the current level; there is no
audio history, latency, or tail. Block splitting does not change results.

The host must select and route the tap destination and accumulate it into any
destination mix. Send Tap does not pick a destination track and does not
replace mixer sends. The host must also decide how bypass and slot mix affect
tap delivery. No effect enum, project, engine, editor, importer, or bindings
integration is included, and the final tree has no `pub mod sendtap;` line.

The processor stores only its parameter struct. Construction and preparation
need no heap buffer. `process`, `set_params`, `reset`, and `send` do not
allocate, reallocate, free, lock, or perform IO.

For validation, re-read `crates/windfall-dsp/src/lib.rs`, temporarily add only
`pub mod sendtap;` immediately before the test command, and run from Git Bash
(`C:\Program Files\Git\bin\bash.exe`) with the MSVC environment:

```bash
source scripts/msvc-env.sh
rustfmt --edition 2024 crates/windfall-dsp/src/sendtap/*.rs
cargo test -p windfall-dsp --lib sendtap
```

Afterward delete only the temporary module line, preserving concurrent edits;
never restore a backup over `lib.rs`. Tests cover dry unity, tap gain, exact
silence, parameter sanitization and metadata, irregular block splits, and
reset. The allocator test compiles the production library and an isolated
probe executable using Cargo's dependency directory, avoiding the existing
library-test allocator. A positive control verifies alloc, alloc_zeroed,
realloc, and free detection; callback counts must be `[0, 0, 0]`. Rustc and
the initialized MSVC environment must remain available. Temporary probe
artifacts are removed on exit.

## Host integration, 2026-10-10

The sections above describe the processor as first delivered, before any host
used it. Since then:

- `pub mod sendtap;` is in the crate, and the effect is registered as
  `EffectKind::SendTap`.
- The engine routes the tap. A saved route (`Mixer.effectOutputs`, command
  `SetEffectOutput`) sends the copy from the effect's place in the chain,
  before the fader, to another mixer track, with its own delay compensation.
- For that path the processor has `send_frames`, which glides to a changed
  level over 5 ms and starts on the level after `prepare` or `reset`. `send`
  is unchanged and still applies a level at once.
- The slot's switch and mix scale what is sent: a slot that is off sends
  nothing.

Design, tests and what remains are in `docs/qa/2026-10-10-mixer.md` under
`fx-fruity-send`. Nothing there was executed after the no-build rule; read its
first section before relying on it.
