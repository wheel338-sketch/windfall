# Windfall factory sounds

These are the sounds Windfall ships with. `Drums` holds a drum kit and `Bass` holds three bass notes for the sampler. `Loops` holds eight original two-bar stereo arrangements across several genres, source tempos and minor keys. Every WAV is 48 kHz, 24-bit PCM. The one-shots are mostly mono; the wide clap, cymbals and loops are stereo.

All of it is public domain under CC0 1.0. See `LICENSE.md`.

## Do not edit these files

The `windfall-factory` crate generates this whole folder, including this file. A test in that crate fails if the folder differs from what the generator writes by a single byte, or if it holds a file the generator does not know about.

To change a sound or add one, edit `crates/windfall-factory/src/sounds`, add new sounds to the table in `crates/windfall-factory/src/lib.rs`, then regenerate from the repository root.

To change a loop, edit `crates/windfall-factory/src/loops.rs`. `Loops/manifest.json` records each asset's ID, path, genre, tempo, key (C=0), minor mode, bars and generator provenance. Every loop uses only Windfall's own oscillator and drum synthesis; no external audio enters the generator. The desktop Loops browser auditions at source tempo and can resynthesize at project tempo and a selected minor key when inserting a clip. Fitting does not change pitch with tempo, and the resulting normal audio clip is one undo step. Later project-tempo changes do not rerender it automatically.

```
cargo run -p windfall-factory --release -- generate content/factory
cargo run -p windfall-factory --release -- verify content/factory
```

`generate` overwrites files and never deletes any, so remove the file of a sound you renamed or dropped by hand. `verify` lists every missing, changed or unexpected file and exits with an error if it finds one. On Windows, run `source scripts/msvc-env.sh` in Git Bash first.

## Bass tuning

The three bass sounds are recorded at C2, which is 65.406 Hz, MIDI note 36, with A4 at 440 Hz. Set the sampler's root key to a C and they play in tune. With the root at MIDI note 60, key 60 sounds C2.

## Why the files are checked in

The generator is deterministic. It uses fixed noise seeds and computes sine, exponential and tanh itself from plain 64-bit float arithmetic, because the C math libraries behind the standard functions disagree in the last bit from one platform to the next. On any machine that follows IEEE 754 it should write the same bytes.

"Should" is not a guarantee for every compiler and CPU, and a build should not need to run a synthesizer. So the WAV files are in the repository, and the app ships those.
