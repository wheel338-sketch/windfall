# Drum synthesis seam

## Current application checkpoint — 2026-10-11

All foundation kinds below are registered/hosted/persisted. Membrane and all
sixteen Drum Rack pads now play an original passive circular modal/cavity model:
normalized Bessel-zero frequency ratios, radial strike position, positive
frequency-dependent damping, bounded excitation and a cavity body contribution.
Membrane appends strikePosition/damping/body at indices 7-9; Drum Rack preserves
indices 0-64 and appends those three controls per pad at indices 65-112.

`LayerKick` / **Layer Kick** is a separate append-only kind for inst-bassdrum:
pitchHz, decayMs, pitchDropSemitones, pitchDecayMs, click, level, sampleMix,
sampleTune and sampleStart. Optional InstrumentAsset::Pcm data is decoded,
downmixed and bounded on a worker, held immutable, and played by eight sample
voices alongside the swept Kick core. Sample-only measured reference, missing-
layer core identity, fixed-storage voice-steal/stop continuity and real public
history/disk/live-offline acceptance are written. Core Kick remains distinct.

Drum Voice now accepts InstrumentAsset::DrumKeys: sorted unique MIDI 0-127
patches, one four-recipe patch per programmed key. Keys select patches with
absolute tuning; a missing key uses channel defaults and key tracking. Mode
and key-program controls latch on hits; channel master level remains live.
The eight-voice cap and deterministic reset seeds are preserved.

Purpose-built single membrane, sixteen-pad, per-key drum and sample-layer
editors use the existing controls/history/automation and audition contracts.
Prepared arrays/PCM live outside Copy parameters. Assets save/reopen in projects,
channel presets and instrument plugin presets. All acceptance is **written,
not executed**; see [hand-off](../../qa/2026-10-11-inst-additive-physical-drum.md).
The remaining text is the historical foundation reference; its two-mode
model and BassDrum exclusion are superseded by this checkpoint.

Implemented in `crates/windfall-dsp/src/drums/`. All four processors implement
the existing `Instrument` trait; all parameter types implement `ParamSet`.
Registry, project/command persistence, UI and parity accounting are integration
owner work and were not changed by this module.

## Public types and suggested registry names

- `drums::Membrane`, `MembraneParams`: suggested `InstrumentKind::Membrane`,
  product name **Membrane**. One tunable voice with eight circular membrane modes and a cavity,
  pitch decay, body decay, tone and noisy snap. MIDI 60 gives `pitchHz`;
  other keys transpose in semitones. Retriggers replace the single voice.
- `drums::DrumRack`, `DrumRackParams`, `DrumPadParams`: suggested
  `InstrumentKind::DrumRack`, product name **Drum Rack**. MIDI 36..51 select
  pads 0..15; other keys are ignored. Each pad has independent `pitchHz`,
  `decayMs`, `tone` and `noiseMix`. Pitch does not track the pad key. Defaults
  differ across pads. One voice per pad; retriggers replace that pad's voice.
  The mapping is fixed, with no inferred played-range mapping.
- `drums::Kick`, `KickParams`: suggested `InstrumentKind::Kick`, product name
  **Kick**. Fast semitone pitch sweep into a body sine and a separate 2 ms
  filtered noise click. MIDI 36 gives `pitchHz`. Eight voices, replacing the
  oldest sounding voice on overflow.
- `drums::DrumVoice`, `DrumVoiceParams`, `DrumMode`: suggested
  `InstrumentKind::DrumVoice`, product name **Drum Voice**. Eight voices,
  replacing the oldest on overflow. `DrumMode::{Kick, Snare, Hat, Tom}` is
  latched per hit; mode changes affect new hits, and continuous settings
  glide on sounding voices. MIDI 36 is the tuning reference. Kick uses half
  the base pitch, a fast pitch sweep and a click. Snare combines two shell
  modes with bright filtered wire noise at 60% of base duration. Hat combines
  high-passed noise and inharmonic sine ring modulation at 25% duration.
  Tom uses two body modes, a slower shallow pitch sweep and a quiet strike.

All processors expose `MAX_VOICES` and `LATENCY_SAMPLES`. Parameter types
derive `Copy`, serde with camelCase and missing-field defaults, and `ts_rs::TS`.
`DrumMode` serializes as `kick`, `snare`, `hat`, `tom`. Nonfinite floating
parameters revert to defaults; finite values clamp to descriptor ranges.
Ranges, units and defaults are in descriptors and field documentation.
Rack indices 0..63 are pad-major groups of four: pitch, decay, tone, noise
mix. Index 64 is master level. JSON paths are `pads.0.pitchHz` through
`pads.15.noiseMix`, then `level`. Preserve these indices in automation.

## Parity scope

- `inst-drumaxx`: Membrane supplies the single synthesized voice;
  Drum Rack supplies the sixteen-pad synthesized workflow.
- `inst-drumpad`: the single Membrane editing/audition workflow.
- `inst-fruity-kick`: Kick's measured pitch-drop/body/click behavior.
- `inst-fruity-drumsynth-live`: Drum Voice's four distinct selectable recipes.

These mappings establish core DSP behaviors, not complete application/UI,
vendor preset or physical-model compatibility. Do not map Membrane to
`inst-bassdrum`; no BassDrum-specific/sample-layer completion is established.
Do not claim `inst-fruity-pad-controller-fpc`: this rack synthesizes audio and
does not load samples, zones, layers or sample-pad programs.

## Runtime limits and behavior

- State is inline and fixed-size: 1 / 16 / 8 / 8 voices respectively.
  Preparation, reset, notes, parameter updates and processing require no heap
  allocation/free, locks, waits or I/O. Overflow replaces a sounding voice;
  voice counts never exceed the published caps.
- Sample rates clamp to 8..384 kHz; nonfinite rates use 48 kHz. No block
  scratch buffer is needed; processing may exceed `max_block`. Equal-length
  stereo buffers are required by `Instrument`. Empty buffers preserve state.
- Output is dual mono. Latency and tail after the last voice are zero.
  Simultaneous hits may sum above unity; the host supplies channel headroom.
  Frequencies saturate at the shared oscillator's 0.249-cycle increment cap.
- Continuous controls glide with a 10 ms time constant per sample. Values
  set before the first nonempty block after prepare/reset apply immediately.
  Equal sample-offset events give bit-identical audio across partitions.
  Noise seeds are deterministic and reset restores the seed.
- `decayMs` is the body's finite exponential duration to exact silence;
  the second mode and strike have shorter envelopes. Snare/hat apply the
  duration ratios above. Key release uses a 30 ms fade; `all_notes_off` uses
  4 ms, including already releasing voices. Natural decay may finish sooner.
  Retrigger/steal continuity uses at most 4 ms of endpoint correction without
  adding a second voice or an unbounded tail.
- Velocity clamps to 1; zero, negative and nonfinite velocity release
  matching notes. Positive-velocity keys above 127 are ignored. Key release
  affects all matching voices. Instance identity, per-note pan/expression/
  tuning, tempo sync, choke groups, samples, preset conversion and specialist
  editors are not implemented; inherited expression calls use key/velocity.

## Verification

Tests measure kick early/late audio periods at 44.1/48/96 kHz, snare/hat
high-frequency energy versus kick, differences among all four modes, rack
pad differences and an isolated audible pad edit, finite extreme output and
bounded note storms, decay/release/choke/reset silence, descriptor/default
and camelCase persistence, and exact partition invariance through automation
and note changes.

Normal check: `cargo test -p windfall-dsp --lib drums`, in Git Bash after
`source scripts/msvc-env.sh`, with `TS_RS_EXPORT_DIR` set to a task-specific
temporary native path. At implementation time this was blocked by unrelated
unfinished DSP modules, including missing multiband/spatial files and drive
serialization errors. No changes were made to those modules.

`drums/verify.rs` is a standalone test root importing the real instrument,
parameter, synth, expression and block modules. It additionally installs a
thread-local counting allocator to exercise parameter updates, notes, voice
overflow, processing, releases, reset and all-notes-off on all four types.
It is not included in the production module or normal library test root.
The following check passed **14 tests**, including six TypeScript exports and
zero observed allocations/frees. Artifact filenames are Cargo's dependencies
in this checkout; use current Cargo `--extern` artifacts if caches change.

```bash
source scripts/msvc-env.sh
task_bindings="$(mktemp -d)"
export TS_RS_EXPORT_DIR="$(cygpath -w "$task_bindings")"
rustc --edition=2024 --test crates/windfall-dsp/src/drums/verify.rs \
  -C opt-level=1 -L dependency=target/debug/deps \
  --extern serde=target/debug/deps/libserde-7d86f515ddf9c160.rlib \
  --extern serde_json=target/debug/deps/libserde_json-5a104ecb2b68c0be.rlib \
  --extern ts_rs=target/debug/deps/libts_rs-31273af07971afb5.rlib \
  --extern windfall_core=target/debug/deps/libwindfall_core-fa6846a083998a08.rlib \
  -o target/drums-verify.exe
target/drums-verify.exe drums
```

Only owned Rust files were rustfmt formatted. No workspace suite, UI tests,
CI, commits, pushes, PRs or parity registry changes were performed.
