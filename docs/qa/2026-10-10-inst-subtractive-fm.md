# Subtractive, macro-hybrid and FM instruments: hand-off, 2026-10-10

Stream: roadmap families I1 and I2. Branch `worktree-agent-abec4265b5432892e`.

**Nothing in this document has been compiled or run.** The stream was
edit-only. Every test named here is "written, not executed". Treat the code as
a first draft that the parent must build, and expect compile errors.

No parity status is changed here. `docs/parity/parity.json` and
`docs/parity/PARITY.md` are untouched.

## Row map

Each row gets its own `InstrumentKind`, its own settings struct and its own
voice structure. Nothing is an alias or a preset of another kind. The existing
`MatrixFm`, `RingHybrid`, `TripleOsc`, `WaveLane` and `MacroVoice` kinds are
left exactly as they were, so their accepted tests are unaffected.

| Row | Kind (serde tag) | Windfall name | State |
| --- | --- | --- | --- |
| `inst-kepler` | `VintagePoly` (`vintagePoly`) | Vintage poly | not started |
| `inst-kepler-exo` | `DualPoly` (`dualPoly`) | Dual vintage poly | not started |
| `inst-poizone` | `CompactSynth` (`compactSynth`) | Compact synth | not started |
| `inst-sawer` | `VintageSaw` (`vintageSaw`) | Vintage saw | not started |
| `inst-minisynth` | `PocketSynth` (`pocketSynth`) | Pocket synth | not started |
| `inst-gms` | `GrooveSynth` (`grooveSynth`) | Groove synth | not started |
| `inst-toxic-biohazard` | `HybridSeq` (`hybridSeq`) | Hybrid sequencer | not started |
| `inst-sytrus` | `MatrixSynth` (`matrixSynth`) | Matrix synth | not started |
| `inst-flex` | `PackSynth` (`packSynth`) | Pack synth | not started |

The per-row sections below are filled in as each instrument lands.

## Shared code (landed)

`crates/windfall-dsp/src/polysynth/`:

- `core.rs`: `Controls` (5 ms smoothed settings, with a "changed this sample"
  flag so envelopes are reconfigured only when a setting moves), `Notes` /
  `Note` (fixed 16-slot allocator: polyphony limit, oldest-released-first
  stealing with a 5 ms de-click ramp, mono legato at a limit of one, constant
  time glide, 5 ms stop fade), `Ladder` (zero-delay-feedback four-pole ladder
  with saturated feedback and pole-mix outputs), `SatSvf` (saturating
  state-variable filter), `Drift`, `drive`, phase and pan helpers.
- `fx.rs`: `Chorus` (also the flanger), `StereoDelay`, `Phaser`, `Reverb`.
  Lines are allocated in `prepare`. With mix at zero each returns its input
  bit-for-bit, which the instruments rely on for exact silence after release.

## What the parent must regenerate

- `scripts/gen-bindings.sh` (new `InstrumentKind` variants, new parameter
  structs and enums, `descriptors.json`).
- `scripts/build-sim.sh` (the WASM the mock backend runs).

## Least-sure spots, shared code

- `core.rs` uses `if a && let Some(x) = ...` chains (edition 2024, Rust 1.88+).
- `Note::finish` frees a slot by assigning `Note::IDLE`; instruments must reset
  their own per-slot state when they see `!note.active` afterwards.
- Strict Clippy: helpers in `core.rs` that end up unused by any instrument will
  warn as dead code and need deleting.
