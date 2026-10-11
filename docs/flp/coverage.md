# FL project import coverage

Verified 2026-10-07 on Windows/MSVC, with the crate also built for
wasm32-unknown-unknown. This is evidence of structural import, not a claim of
identical playback or support for every FL Studio release.

## Sources and licensing

See [crate notices](../../crates/windfall-flp/NOTICE.md) for exact pinned revisions,
copyright notices, license evidence and dependency licenses. PyFLP, DawVert and
FLParser grant GPL-3.0-or-later; the LMMS v1.0.3 importer grants GPL-2.0-or-later.
PyFLP's metadata says GPL-3.0, but its relevant source headers explicitly permit
version 3 or later. The crate declares the workspace GPL-3.0-or-later license.
No GPL-3.0-only constraint was found in these sources. Helpers are serde and
thiserror (MIT OR Apache-2.0), ts-rs (MIT), and test-only proptest (MIT OR Apache-2.0).

No FL Studio binaries or plugins were decompiled or disassembled. Format layouts
and formulas come from the credited open implementations. Test streams are
constructed by the original CC0-1.0 writer in tests/common/mod.rs. No Image-Line
projects, presets, samples, artwork or third-party binary fixtures are committed.

## Real-file verification

Files were read from the GPL-licensed [PyFLP test asset tree](https://github.com/demberto/PyFLP/tree/f937126b888ce94271bfea631b89166c74056530/tests/assets),
cloned into `C:/Users/ewhee/AppData/Local/Temp/windfall-flp-sources/PyFLP`, outside
this checkout. Nothing from that folder is distributed by Windfall. The inspected
revision is f937126b888ce94271bfea631b89166c74056530. The inspection example prints
counts, report outcomes and the actual Project::check result.

| PyFLP asset | FL version | Parse | Channels | Patterns | Notes | Inserts | Playlist items | Unknown ids | Conversion |
|---|---|---|---:|---:|---:|---:|---:|---|---|
| FL 20.8.4.flp | 20.8.4.2576 | success, 0 diagnostics | 19 | 5 | 48 | 127 | 22 | 31, 35, 36, 37, 38, 39, 40, 157, 158 | Project::check = Ok; 18 rack channels, 5 patterns, 112 notes after layer expansion, 21 clips, 0 automations, 15 retained unsupported plugin states |
| patterns/multi-channel.flp | 20.8.4.2576 | success, 0 diagnostics | 2 | 1 | 2 | 127 | 0 | 31, 35, 36, 37, 38, 39, 40 | Project::check = Ok; 2 channels, 1 pattern, both notes, 0 clips |

All 48 source note records in the larger asset are retained. Four layer notes
expand to their children; notes targeting audio/automation channels are kept on
silent samplers. This produces 112 Windfall notes. One unsupported automation
playlist item is dropped and reported; the other 21 item placements survive.
The output will not sound like the source without missing instruments/effects
and samples. Verification supplied no sample directories, so unresolved source
paths are expected. Empty playlist track records and touched insert records
are imported too; counts below include them.

Per-category reports for the larger asset (exact / approximated / placeholder /
dropped): settings 3/0/0/4; channels 0/10/9/0; samples 0/1/0/0; patterns 5/0/0/0;
notes 9/39/0/0; mixer 99/5/0/0; effects 0/0/10/0; playlist 5/516/0/1; other
0/0/0/427. Other includes uninterpreted event occurrences and markers, not
427 lost musical clips. For multi-channel: settings 2/1/0/0; channels 0/2/0/0;
patterns 1/0/0/0; notes 0/2/0/0; mixer 104/0/0/0; playlist 0/500/0/0; other
0/0/0/57. Report counts refer to source items, not expanded layer notes.

The same repository's six intentionally corrupted assets were checked:
invalid-data-magic, invalid-header-magic, invalid-header-size and invalid-ppq
return readable errors; invalid-event-size yields a partial model with one
diagnostic and a checked project; invalid-format preserves the unfamiliar
format in the header rather than panicking. These files do not establish
additional version coverage.

Reproduce, from this checkout in Git Bash:

```sh
source scripts/msvc-env.sh
cargo run -p windfall-flp --example inspect -- "C:/external/PyFLP/tests/assets/FL 20.8.4.flp"
```

No real projects from FL Studio 3, 8, 9, 11, 12, 21, 2024, 2025 or 2026 were
available in the checked source asset tree. Their coverage is unverified.
Generated historical and 21-era fixtures are useful layout tests, not substitutes
for a multi-version real-project corpus.

## Parsed and converted scope

| Feature | Reader evidence | Conversion and limits |
|---|---|---|
| FLhd/FLdt, typed events, unknown ids, version | PyFLP + LMMS; all four event ranges tested | File size capped at 512 MiB; object counts and diagnostics capped; unknown data preserved up to the documented retention cap and all occurrences counted |
| Tempo/fine tempo, signature, text, master controls | Multiple sources; old denominator meaning from PyFLP | Tempo, signature and name imported via commands; unsupported metadata and main pitch reported; master volume folded into master gain |
| Sampler controls, sample paths, cut groups, reverse, window/envelope | Multiple sources; envelope timing is a DawVert fit | External normalized references; variables resolve only from options; unresolved paths stay written; asymmetric groups, stretching, looping, LFOs and unsupported effects reported |
| Generator/wrapper/layer | Wrapper records from PyFLP/DawVert; type 4 interpretation follows DawVert | Three-oscillator source synth maps approximately to subtractive synth; unsupported plugins are silent samplers with raw wrapper state in Conversion.plugins and Project.retainedPlugins; layers expand notes with cycle protection |
| Notes/patterns/legacy steps | Modern 24-byte and historical 20-byte generated records; real 20.8.4 records | Exact ticks when representable; nearest-tick rounding otherwise; step-grid pattern lengths; fine pitch, release, modulation, slide duration and note-color MIDI channels mapped; source note-edit groups and exact slide chord targeting remain unsupported |
| Mixer slots, mute/solo, routing/sends | Multiple implementations; generated controls and routing fixtures | Command-enforced cycles refused; 128 tracks, 10 effects per track; EQ/compressor/reverb/delay have partial parameter translations; limiter/delay 3 use defaults; unsupported effect states retained separately |
| Playlist tracks/clips/audio | 32-byte and generated 60-byte records; real 20.8.4 | Pattern position/length/offset/mute imported; audio references, reverse, pitch, approximate fades/gain imported; track name/mute/color imported; source relative height fitted to 48 CSS pixels per unit and clamped to 18..160; channel offsets disputed and reported |
| Automation points/links | PyFLP/DawVert/FLParser layouts; generated link tests | Channel volume/pan, track volume/pan, existing send gain, effect mix, understood effect parameters and tempo; tension fitted/approximate, hold mode supported; unsupported targets/controllers/formulas/smoothing reported; instrument-parameter links currently unsupported |
| Markers and extra arrangements | Generated layouts | Selected named markers and meter changes mapped; independent clip/track layouts mapped through AddArrangementLayout; alternate timeline markers/meters remain omitted because the project timeline is shared |

Automation and channel clip offsets remain a disputed area: open implementations
use different units. Audio offsets follow DawVert's four units per quarter note
before stretching; automation offsets use the quarter-note interpretation.
Nonzero offsets are reported as approximate. Bent curve shape and fader gain
fits do not imply acoustic equivalence. Native source parameters outside the
Windfall processor's range are sanitized through the same commands as the app.

## Test and build evidence

Final command: `cargo test -p windfall-flp --quiet` (with a unique external TS_RS_EXPORT_DIR).
72 unit tests + 41 model/writer tests + 15 conversion tests = 128 passing tests;
0 failures and 0 ignored tests; doc tests: 0. Property tests use 128 cases each:
constructed notes round-trip writer/reader and convert deterministically to a
checked project; arbitrary hostile event bytes never panic. Explicit PPQ tests
cover 96, 192, 384, 480, 960 and odd bases. 384 PPQ is not a divisor of 960:
odd source ticks require rounding; even source ticks are exact.

Required checks: `cargo clippy -p windfall-flp --all-targets -- -D warnings`,
`cargo fmt -p windfall-flp -- --check`, and
`cargo build -p windfall-flp --target wasm32-unknown-unknown` all pass.
Those checks verified the reader/converter stage. The desktop workflow is now
implemented and documented in [desktop-import.md](desktop-import.md).

## Integration API

```rust
pub fn parse(bytes: &[u8]) -> Result<FlpProject, FlpError>;
pub fn convert(flp: &FlpProject, options: &ConvertOptions) -> Conversion;
pub fn import(bytes: &[u8], options: &ConvertOptions) -> Result<Conversion, FlpError>;
```

Conversion contains Project, ImportReport and Vec<PluginPlaceholder>. The project
also stores unsupported states in the additive `retainedPlugins` field. The
shell now reads bytes, checks the conversion, resolves and decodes samples for
review, and opens a fresh unsaved Document only after confirmation and the
shared replacement ticket checks. IPC is `flp_preview`, `flp_open` and
`flp_cancel`; the report dialog groups counts and expandable diagnostics. The
Imported sounds status button exposes retained metadata after save/reopen.
The browser mock uses the same converter through wasm. See
[desktop-import.md](desktop-import.md) for the workflow, bounds and tests.
This crate does not check referenced sample files itself or implement hosted
plugin recovery; those responsibilities remain with the shell and a future
compatible state decoder.

## 2026-10-11 edit-only fidelity extension (not verified)

Independent arrangements own distinct clip IDs and lane settings, including
identical placements, and the selected arrangement is restored after import.
Project author/genre/comments and supported source timebase are mapped.
Dock bits follow [PyFLP mixer.py](https://github.com/demberto/PyFLP/blob/master/pyflp/mixer.py).
Per-channel swing follows [PyFLP channel.py](https://github.com/demberto/PyFLP/blob/master/pyflp/channel.py);
nonlinear time shift remains reported, not decoded. Enabled zero-level routes
become unity detector-only sidechains (approximate; source automation that changes
a zero-level route into audible routing is not preserved). A documented 16-byte
send state can supply an explicit destination and approximate tap gain;
negative/relative destinations, send pan and dry controls remain omitted.
Mapped instruments/effects now retain their original opaque states too. They
are archival data, never passed to an unrelated native plugin host.

Sampler dimensionless stretch speed maps to prepared spectral duration when
its mode is non-resampling, requested time is zero, and its ratio is 0.25..4.
Stretch pitch is added to sampler tune. All MIDI keys are requested for preparation;
the existing strict memory budget can refuse large sources. Requested duration,
absolute stretch-time resolution, source algorithms and sample FX remain open.
Desktop preview resolves requested WAV smpl loops after decoding, with bounds
relative to the trimmed region and review warnings for unsupported loop behavior.
No source-only loop bounds are guessed as whole-sample loops.

Tests added/updated in windfall-flp: **written, not executed**. The historical
real-file table above is unchanged evidence from earlier runs, not validation of
this extension. Full-row claim remains partial; corpus, plugin-state decoding,
controller mappings and precise nonlinear timing still need work.
