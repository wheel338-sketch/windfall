<img src="assets/icon.svg" width="64" height="64" alt="">

# Windfall

Windfall is a free, open-source digital audio workstation in the style of FL Studio. It is a native desktop app: the audio engine is Rust, the window is Tauri 2, and the interface is React with shadcn.

Windfall 1.0.0 is the first stable release. It has a channel rack and step sequencer, a piano roll, a playlist, a mixer with effect chains, sends and sidechains, dozens of built-in instruments and effects, CLAP and VST3 plugin hosting, audio and MIDI recording, export to WAV, FLAC, OGG, MP3, WavPack and M4A, stem separation with a model you supply, a small scripting language, FL Studio project import, and an English or Spanish interface. [The release notes](docs/RELEASE-NOTES-1.0.0.md) list what is in it and its known limitations, and [the parity tracker](docs/parity/PARITY.md) lists every FL Studio feature and how Windfall covers it.

Windfall is not affiliated with Image-Line. It contains no FL Studio code, samples, presets or artwork. It is licensed under GPL-3.0.

## Download

Windows 10 and 11 (64-bit) users can download the installer from [GitHub Releases](https://github.com/wheel338-sketch/windfall/releases). The installer is not code-signed yet, so Windows SmartScreen may warn before running it; choose **More info** and then **Run anyway**. macOS and Linux builds are not published yet; the code builds on those systems from source.

## Build from source

You need Rust, Node 24 or newer, and pnpm. On Windows you also need the Visual Studio C++ build tools, and on Linux the WebKitGTK and ALSA development packages that the CI workflow installs.

```bash
# Windows, in Git Bash: load a Visual Studio environment that can link
source scripts/msvc-env.sh

cd apps/desktop
pnpm install
pnpm tauri dev
```

To work on the interface without building the engine, `pnpm dev` runs it in a browser against a simulated backend at http://localhost:1420.

## Layout

| Path                                | What it is                                                                                                    |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `crates/windfall-core`              | Audio buffer and unit helpers shared by everything                                                            |
| `crates/windfall-project`           | Project model, edit commands, undo history, `.windfall` file format                                           |
| `crates/windfall-archive`           | Portable ZIP schema, bounded validation and staged audio extraction                                           |
| `crates/windfall-analysis`          | Analysis jobs: denoise, dereverb, pitch and tempo tools, audio-to-MIDI, stem separation |
| `crates/windfall-engine`            | Realtime audio engine, offline renderer and bounded analyzer foundation                                       |
| `crates/windfall-flp`               | Reads FL Studio projects and converts musical structure with an import report                                 |
| `crates/windfall-codec`             | Reads WAV, AIFF, FLAC, MP3, OGG and WavPack; writes WAV, FLAC, OGG, MP3, WavPack and M4A                    |
| `crates/windfall-midi`              | Reads and writes MIDI files; project import plans and playlist export                                         |
| `crates/windfall-dsp`               | Effects and synth DSP                                                                                         |
| `crates/windfall-plugin-host`       | CLAP/VST3 audio hosting, isolated scanning and Windows native editors                                         |
| `crates/windfall-stretch`           | Pure Rust streaming/offline time-stretch, pitch-shift and loop tempo helpers                                  |
| `crates/windfall-ipc`               | Types the engine, shell and interface exchange                                                                |
| `crates/windfall-factory`           | Generates the factory sounds                                                                                  |
| `crates/windfall-sim`               | The project document compiled to WebAssembly, which the simulated backend runs                                |
| `content/factory`                   | Factory sounds, CC0                                                                                           |
| `apps/desktop`                      | The app: `src-tauri` is the shell, `src` is the interface                                                     |
| `apps/desktop/src/components/audio` | Knob, fader, meter, step grid and other audio controls, MIT licensed and installable as a shadcn registry     |
| `docs`                              | Architecture, parity matrix, performance measurements                                                         |

`docs/ARCHITECTURE.md` explains how the parts talk to each other.

## Checks

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
pnpm --dir apps/desktop test
node scripts/parity.mjs --check
node scripts/check-sim.mjs
```

After changing a Rust type that the interface uses, run `scripts/gen-bindings.sh`.

The simulated backend edits projects with the real Rust code, built to WebAssembly and checked in. After changing `windfall-project`, `windfall-core` or `windfall-sim`, run `scripts/build-sim.sh` and commit the two files it writes. It needs `rustup target add wasm32-unknown-unknown` once.

## License

Windfall is licensed under the GNU General Public License, version 3 or later. See `LICENSE`.

Two parts carry their own, more permissive terms so other projects can reuse them: the audio control kit in `apps/desktop/src/components/audio` is MIT, and the factory sounds in `content/factory` are CC0.
