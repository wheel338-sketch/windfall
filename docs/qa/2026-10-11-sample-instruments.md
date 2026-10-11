# Sample instruments — 2026-10-11

Branch: `gpt/w3-sample-instruments`, base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.
All implementation and tests below are **written, not executed**. No build,
test, package-manager command, device check or sub-agent was run.

## Current implementation

The new `windfall-dsp::sample_bank` path leaves the older fixed-storage
`zones` seam intact. Six appended kinds (`MultiSampler`, `SamplePlayer`,
`SoundFont`, `PadBank`, `PianoKeys`, `TextVoice`) have small Copy controls;
PCM lives in immutable worker-prepared banks. Project channels optionally
save zone SampleId references, SF2 path/bank/program references or versioned
own-source recipes. Checked mapping/load commands use existing channel
edits, history and patches; mapping editing is rejected for the player.

Banks bound zones to 256, PCM to 256 MiB and layer voices to 128. Selection
supports inclusive key/velocity ranges, independent fades, additive layering,
round robin, chokes, stereo sources, tuning, loops and volume envelopes.
Missing sources and unsupported content return preparation errors. Voices
own only indexes; entire bank/processor ownership retires with engine plans.

The dependency-free Rust SF2 reader follows RIFF hydra links and validates
record widths, chunk/padding sizes, all references, sample bounds, and ranges.
It extends the existing own reader with real-sized PCM, address offsets,
scale tuning, exclusive class and volume-envelope generators. Unsupported
modulators, filters/LFO generators, 24-bit and unsupported sample types fail
explicitly. This is a documented subset, not general SF2 compatibility.

The piano material is generated mathematically in `sample_bank/sources.rs`:
15 roots and two velocity layers, struck partials with slight inharmonicity
and per-partial decay, plus an electric-keyboard recipe. No proprietary
samples, voices, presets or new dependencies. Source recipe version is 1.
The robotic English text renderer uses newly authored grapheme/digraph rules,
pulse/noise excitation and formant resonators; pitch and duration are independent.
Native rendering saves Float32 WAV in app-owned storage and attaches its
SampleId in one stale-guarded undo batch. Failed work deletes its temporary WAV.

## Tests and integration commands

`crates/windfall-dsp/src/sample_bank/tests.rs` (written, not executed):
inclusive boundaries/fades; additive layers/missing keys; round robin/chokes;
bounded stealing/emergency retirement; loop/envelope partition equality;
invalid PCM/mappings/loops; speech determinism/errors/cancellation; own piano
source coverage; constructed real SF2 PCM/preset mapping; every truncated
prefix/hostile links; envelope/tuning/preset offsets.

Integrator, in Git Bash with the established MSVC environment:

```bash
source scripts/msvc-env.sh
export TS_RS_EXPORT_DIR="$(cygpath -w "$(mktemp -d)")"
cargo test -p windfall-dsp --lib sample_bank
cargo test -p windfall-dsp --test dsp params
cargo test -p windfall-project --lib --tests
cargo test -p windfall-engine --lib --tests
cargo test -p windfall-desktop --lib
cargo clippy -p windfall-dsp -p windfall-project -p windfall-engine -p windfall-desktop --all-targets -- -D warnings
# Regenerate bindings, descriptors and sim using the repository workflow.
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop test src/features/sample-instruments src/features/params src/lib/ipc/sim/document.test.ts --maxWorkers=4
```

## Remaining work / least-sure spots

Editors, public engine/history/retirement acceptance, peripheral adapters and
General MIDI wiring are being added in subsequent coherent commits.
Check registry exhaustive matches and Channel literals first. Reconcile
append-only enum lengths with the parallel instrument streams. Check the
new `SamplerPreparationError::Asset` in downstream exhaustive matches.

Current limitations to keep explicit: changing a bank constructs a fresh
instrument; sustaining voices across an asset replacement is not yet
implemented. The 256 MiB limit counts the candidate project, not all
simultaneously retained old plans or concurrent preparation workspaces.
SF2 defaults/modulators/stereo interoperability need broader external fixtures.
Speech quality is intentionally robotic and limited to English ASCII rules.
No row is claimed accepted from unexecuted source.

## Generated artifacts

Regenerate bindings/descriptor catalog/WASM; never hand-edit generated files.
Changed: `Channel`, `Command`, `InstrumentKind`, `InstrumentParams`.
New: `SampleInstrumentAsset`, `SampleZone`, `BankLoop`, `SoundFontPreset`,
six new instrument params named above.
