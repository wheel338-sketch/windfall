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
| Notes/patterns/legacy steps | Modern 24-byte and historical 20-byte generated records; real 20.8.4 records | Exact ticks when representable; nearest-tick rounding otherwise; step-grid pattern lengths; fine pitch, release, modulation, slide duration and note-color MIDI channels mapped; source note-edit groups persist independently of color and drive grouped editor selection; same-color slides anchor the highest sounding pitch at onset and preserve note endings |
| Mixer slots, mute/solo, routing/sends | Multiple implementations; generated controls and routing fixtures | Command-enforced cycles refused; 128 tracks, 10 effects per track; EQ/compressor/reverb/delay have partial parameter translations; limiter/delay 3 and the defined native layouts have disclosed stand-in translations; unsupported effect states retained separately |
| Playlist tracks/clips/audio | 32-byte and generated 60-byte records; real 20.8.4 | Pattern position/length/offset/mute imported; audio references, reverse, pitch, approximate fades/gain imported; track name/mute/color imported; source relative height fitted to 48 CSS pixels per unit and clamped to 18..160; channel offsets disputed and reported |
| Automation points/links | PyFLP/DawVert/FLParser layouts; generated link tests | Channel volume/pan, track volume/pan, existing send gain, effect mix, understood effect parameters and tempo; tension fitted/approximate, hold mode supported; unsupported targets/controllers/formulas/smoothing reported; instrument targets lacking public semantic numbering receive device/parameter-specific SCOPE evidence; offsets are never substituted for numbers |
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

## 2026-10-12 edit-only pattern automation and alternate timelines (not verified)

Event 223's already-parsed 12-byte records now become held pattern event lanes
for channel volume/pan, insert volume/pan, existing send gain, slot mix and
parameters in the existing understood effect translation tables. Source integers
use the established source-unit conversions, not the automation-clip normalized
value path. Points sort by rescaled tick, the last source record wins a duplicate
tick, and existing command point/lane/range limits remain enforced. Conversions
are reported approximate; unknown targets, pattern tempo events, values without
an established conversion and overflow remain reported omissions. Original event
records remain in the reader model; unsupported processor state remains in the
existing opaque plugin archive. This does not claim recovery of every controller
or an instrument's opaque patch parameters.

Independent imported arrangements now retain each supported named/meter timeline.
The additive, default-empty ArrangementBook.timelines field opts into snapshots;
old v1 files continue to share their one timeline. CaptureArrangementTimeline is
appended to Command and uses checked edits. Switching snapshots current edits and
restores the selected layout; copying allocates fresh marker/meter identities;
deleting the active layout restores its surviving neighbour. Ordinary timeline
editing, engine compilation, export and patching consume playlist.timeline as
before. Unsupported navigation kinds still receive warnings; no end positions
are guessed. Single-arrangement imports retain their legacy allocator behavior.

Synthetic event-byte, checked history, JSON persistence and hostile-range tests
were written. Nothing was built, generated or run. Channel nonlinear time shift,
undocumented source state layouts, source formulas/smoothing and additional real
version corpus checks remain open. See the 2026-10-12 p2 hand-off for exact tests.

## 2026-10-12 round-four edit-only archival extension (not verified)

The reader now retains the original ordered event stream, excluding personal
licensee event 200, up to 16 MiB. It stops retention at a complete event boundary
and reports overflow while continuing ordinary bounded conversion. Conversion
stores a valid FLhd/FLdt container in the default-empty, additive
Project.retainedSourceData archive (`source: flStudio`, `format: flp-events-v1`).
Owner-selection events, processor state, event-223 native integers, event-227
unknown/formula/controller bytes, nonlinear timing and navigation records are
therefore inspectable after save/reopen, even when their semantics cannot be
translated. Archive bytes are never executed or routed to an unrelated plugin.
The archive is separate from retainedPlugins, so plugin counts stay meaningful.
Project validation bounds its count, names and aggregate payload on reopen.

The obsolete second pattern-control converter was removed; event 223 is now
processed once by the existing bounded, held-point converter, including its
understood effect parameter tables. This prevents duplicate command rejection
warnings and double-counted control losses. No new source processor layout or
controller formula was invented. Core unsupported synthesis, instrument parameter
automation, formulas/smoothing, nonlinear shift/glide and navigation translation
remain software gaps; archival retention is not playback equivalence.

Synthetic tests written in crates/windfall-flp/tests/conversion.rs:
`unsupported_control_controller_timing_and_navigation_bytes_are_durable_with_owner_context`
and `source_event_archive_excludes_licensee_and_stops_at_complete_events_with_a_budget_warning`.
They inspect source event 223, controller 227, timing 89, marker 148, unknown 250,
personal metadata exclusion, archival container equality, round-trip parsed
values, JSON persistence and overflow. Existing recorded-control assertions stay
registered, with an additional assertion against duplicate conversion warnings.
No test/build/generation, proprietary binary inspection, or real-project corpus
check was performed. Real projects from additional releases remain an external
compatibility check, not synthetic-fixture evidence.

## 2026-10-13 round-six builder progress (verified targeted suite)

`cargo test -p windfall-flp --no-fail-fast -- --test-threads=1`: 174 passed,
zero failed (97 unit, 27 conversion, 41 model, 9 independent byte-stream tests).
Full workspace gates are tracked in
[the round-six record](../qa/2026-10-13-r6-formats.md).

| Translated item | Public source, licence and pin | Mapping and remaining limits |
|---|---|---|
| Channel portamento | DawVert c6c8be9e, GPL-3.0-or-later, mi_flp.py | `(slide/1024)^4` sixteenth steps to ms at imported tempo; 0..60,000 ms; engine glide shape is approximate |
| Arpeggiator and echo | PyFLP f937126b channel.py; DawVert c6c8be9e channel.py/mi_flp.py, GPL-3.0-or-later | Direction/range/gate/rate and echo feedback/time/count/pitch mapped; unsupported chords/repeats/slide/random/sticky and echo pan/flags reported individually; arp float/u32 dispute reported |
| Modulation ADSR and LFO tails | Same public channel sources | Pan/cutoff/resonance/pitch ADSR/depth; all optional tail words read; sixteen-step LFO timing anchor mapped, other nonlinear periods and delay/hold/tension/sync reported |
| Fruity Balance native state and automation | PyFLP plugin.py; DawVert fl_studio.ddef/dset and flstudio_univ.pltr | Pan/128, gain/256 approximation; public source parameter 0 pan/1 volume maps by descriptor id to Windfall pan=1/gain=0; clip and recorded automation fixtures; opaque state also retained |
| SimSynth FLP native state | DawVert fl_studio.ddef, flp_dec_plugins.py, n_flstudio_foss.py, same pin/grant | 45 doubles + 76 reserved bytes + flags; oscillator shape/coarse/fine/level/on and amplitude ADSR/gain; other named filter/modulation/unison/chorus fields still opaque; not a standalone .syn reader |
| Arrangement loop/skip/pause | DawVert mi_flp.py action numbers; [public Playlist manual](https://www.image-line.com/fl-studio-learning-content/fl-studio-online-manual/html/playlist.htm), copyrighted documentation | Marker-loop/skip end at next source marker; song-loop ends at playlist data; pause maps unless preceding song-loop suppresses it; missing endpoints reported; start/punch names survive but transport behavior remains open |

All exact full pins and attribution remain in NOTICE.md and the round-six record.
Every fixture is constructed in Rust; no third-party preset/project/audio enters
the repository. Processor layouts not implemented yet, note grouping, controller
formulas/smoothing, instrument parameter numbering, nonlinear timing and start/
punch regions remain software work, not merely archival or external checks.

Round-six final workspace gates pass: format, strict Clippy, **4,716 Rust tests /
0 failures**, typecheck, lint, **6,453 frontend tests / 0 failures**, regenerated
bindings and refreshed/freshness-checked simulator. The native fixture/helper/
export paths must be injected as documented in the round-six record. The initial
missing-environment failure is retained separately; assertions and opt-in test
annotations were unchanged. This passing gate verifies the implemented subset;
the full FLP row remains **not done** with the software omissions listed above.


## 2026-10-13 round-six edit-only continuation (written, not built or tested)

Continues base `0b72d3f2`, after the builder's 174-test stopping point. The
historical passing gates above do **not** validate these edits. Complete test
names and required integration work are in
[the FLP hand-off](../qa/2026-10-13-r6-flp.md). Full pins, licences and exact
source files for every adopted layout/formula are in
[NOTICE.md](../../crates/windfall-flp/NOTICE.md#round-six-native-state-continuation-2026-10-13).

All active DawVert definitions were enumerated, including nested definitions.
FL Slayer, FL Keys, Big Clock, Filter, Spectroman, Wasp/Wasp XT, Formula Controller,
Razer Chroma and Kepler have no entry in the current instrument/effect stand-in
tables or this coverage table and are outside the requested stand-in set.
Their layouts are not misclassified as absent. Existing 3x Osc, Balance,
EQ/compressor/reverb/delay/send translators remain in place. The remaining
intersections are listed individually below. Each scalar is decoded/reported;
fields without a corresponding stand-in control receive a named explanation.
Opaque source state is retained alongside every translated patch.

For a range-only field, scaling is explicitly linear:
`destination_min + clamp((raw-source_min)/(source_max-source_min),0,1) *
(destination_max-destination_min)`, followed by ordinary descriptor sanitization.
The source ranges and exact destination descriptor IDs are in `convert/native.rs`;
reports print the raw value, destination value, rule and range. This is a disclosed
stand-in fit, not a recovered acoustic transfer function. Enum order, automation
numbering and physical units are never inferred from byte positions. Float fields
with no published unit use an explicitly reported identity/bounded stand-in fit.

| Native device with a public layout | Translation and independent synthetic evidence in `round6_native.rs` |
|---|---|
| Plucked! | decay/color -> Pluck decaySeconds/brightness via source 0..256/0..128 ranges; 128 -> 6.05 seconds; normalize/gate/widen have no equivalent Pluck descriptors |
| BooBass | Both 12/16-byte forms -> Finger Bass body/damping/tone, /65535; full bass -> body 1; version is metadata |
| Fruit Kick / Fruity Kick | min cents -> C4 frequency fit, max/min difference -> pitch drop; volume/frequency decay and click range fits; midpoint decay -> 755 ms |
| Fruity DX10 | Operator envelopes 5000*n^6/^2/^3, levels/sustain n^2, ratio 2*floor(n^2*40), fine n^4*180 cents folded by 2^(cents/1200), octave ratios and inactive branches; FanIn stand-in; vibrato=n/5.5, rate=n*127 clamped 20 Hz |
| SimSynth | MIDI cutoff 100+(cut-.5)*40+(kb-.5)*10 -> Hz, emphasis*.8; 4*v^3-second independent SVF ADSR, env*6.4 octaves; documented LFO fit; enabled oscillator levels weight shared pitch modulation; every float/flag named in report |
| Fruity Granulizer | grain-size/density/spray/start range fits; event-196 sample reference attaches to GrainCloud playback asset; midpoint grain size -> 125.5 ms |
| BassDrum | SSLF float array, level/pitch/duration/click/sample ratio fits; fixed filename -> prepared Layer Kick PCM asset (mono/int16, max 32,768 frames, faded truncation); synthetic PCM equality and JSON reopen |
| Fruity Slicer | Header and variable strings/keys/names/reverse/starts; frame starts use public channels/2; pitch/100, disclosed 0..128 -> 0..100ms attack/release; sparse custom-key holes are silent; 100 of 200 stereo frames -> .5 |
| Slicex | Two u16 versions and named master controls, filename/embedded-WAV length; PCM8/16/24/32 and float32 WAV, cue-frame boundaries, mono fallback up to 4,194,304 frames; external WAV cues use existing codec and keep labels; synthetic stereo downmix, boundaries, filename and JSON equality |
| Fruity Center | 4/8-byte enable forms -> actual DC-block slot enable |
| Fruity Mute 2 | Public mute/channel rule -> actual left/right mute values; source gains are inverted to match target mute semantics |
| Fruity Phase Inverter | Source 0..1024 state fitted to binary inversion of both sides (>512); no guessed left/right mask |
| Fruity Stereo Shaper | Signed four-way matrix /12800 -> StereoMatrix; 6400 -> .5 |
| Fruity Soft Clipper | Threshold range -> ceilingDb; postgain has no matching stand-in control |
| Fruity Fast Dist | Pre/threshold/post -> drive/shape/output range fits; midpoint drive -> 18 dB |
| Fruity Fast LP | `(100*(.2+n)^1.5)^2*925/2048`, Q=1+6*n; normalized float 0/.5/1 -> 36.1328125/1549.1943359375/7804.6875 Hz |
| Fruity Free Filter | Published seven-response selector and frequency/Q/gain fits; explicit dataset numbering with converter type/700 and freq/1024 ranges; dataset disagreement and ambiguous zero selector reported; half gain -> 9 dB |
| Fruity Bass Boost | Frequency/gain range fits; half gain -> 9 dB |
| Fruity Blood Overdrive | 32/36-byte forms, preband/color/preamp/x100/postfilter/postgain; midpoint preband -> 2100 Hz |
| Fruity Chorus | Three LFO rate/delay/depth/stereo range fits; midpoint delay -> 15.5 ms |
| Fruity Flanger | rate=5*n^7, delay=20*n^2.58495, feedback/100 and polarity; half rate -> .0390625 Hz |
| Fruity Phaser | Sweep/depth/stereo/feedback/mix range fits; half sweep -> 2.5 Hz |
| Fruity Flangus | Order/depth/rate/delay/spread/mix -> StackedFlanger; half speed -> 2.5 Hz |
| Fruity 7 Band EQ | All seven signed gains /100; -600 -> -6 dB |
| Fruity Stereo Enhancer | Pan/128, width=1-stereo/96, phase offset range fit and inversion choice; -64 -> -.5 balance |
| Soundgoodizer | 8/12-byte amount -> OneKnob; 500 -> .5; source mode has no target mode descriptor |
| Transient Processor | Attack/release/drive fits and actual slot enable; 120/160 -> attack .5 on target -1..1 range |
| Frequency Shifter | Source frequency seventh-power fit, public range selector and mix; half full-range -> 156.25 Hz |
| Spreader | Spread/separation fits, byte tails and actual enable; half spread -> .5 |
| Fruity PanOMatic | Pan/128-1, gain/256, shape/depth/rate fits; 64 -> -.5 pan; no independent amplitude-LFO selector in target |
| Fruity Limiter | All public limiter/compressor/noise words, mirrored words and byte tails; target limiter/compressor/gate controls, disclosed range fits; midpoint ceiling -> -12 dB |
| Fruity Delay 3 | Public 27-word state and time/mix/feedback/modulation/damping fits; tempo sync uses source /192 quarters to ms; feedback midpoint -> .475 |
| Fruity Delay Bank | All eight 64-byte banks and eight 32-byte extras decoded; EchoBank control matrix/enable/filters/modes/times and global /128 gains; first-unit half input -> 1 |
| Multiband Delay | Explicitly unfinished public prefix, all eight arrays of delay/vol/pan decoded; since round seven all eight banks of sixteen bands land on the stand-in's banks (see the round-seven section); 120 float -> 120 ms |
| Pitcher | SSLF floats and key bytes, speed/tuning/harmony fits; reference=415.3+raw*50.9, .5 -> 440.75 Hz |
| Vintage Chorus | Public state and mode/rate/delay/wet/stereo fits; half rate -> 2.5 Hz |
| Vintage Phaser | Public state and rate/min/max/feedback/stereo/mix fits; half rate -> 2.5 Hz |
| Fruity Multiband Compressor | Three threshold/ratio/makeup/time blocks plus crossovers; 500/65536 -> low ratio 1.75531 |
| Tuner | Reference=400+raw/75; 3000 -> 440 Hz; display/reactivity metadata named |
| Pitch Shifter | Explicitly NOT_FINISHED named word/byte prefix including display_speed/on/mono; pitch/formant /100; -700 -> -7 semitones; raw 256-byte section has no public semantic schema and stays opaque |
| Fruity Vocoder | Bounded band count/float array and named tail, envelope/wet/color fits; half attack -> 50.05 ms |
| Fruity Waveshaper | Public aligned header and bounded point records -> 33 knots, tension via disclosed exponential fit, instant via hold; independently computed linear/bent/held values and hostile-count fixture |
| Effector | Commented 13-field candidate prefix decoded as typed fields; wet/X/Y and bypass fit; XY/mix=.25/.75/.5 fixture; model ordering/modulation units/public numbering not established and named as SCOPE |

### Controls, editing and timeline continuation

| Item | Current translation / fixture evidence |
|---|---|
| Native parameter automation | Explicit named dataset `num` only, descriptor ID lookup preserves saved append-only addresses; automation clips and recorded event 223 use same physical mapping. Public normal f32 words 0..1 decoded; non-normal small native integer compatibility retained. No field offset is used as a number. |
| Instrument parameter automation | Device/parameter-specific reports for both clips and recorded targets. SCOPE: decoded generators have no named `num` in the dataset; Sytrus/Harmless param_N ranges do not establish knob meanings. PyFLP parsers and LMMS supply no semantic numbering; original source events remain. |
| Pitch range / channel filter | Wrapper MIDI record1 bytes8..12 -> existing bend-range map, send flag retained; **event83 is resonance**. Modern event219 filter /256, legacy cutoff /1024 and resonance /640; documented cutoff/Q formulas on a private bus, with Mod X/Y clip/recorded automation. |
| Remaining keyboard/sample fields | Event142 signed fine tune, main-pitch participation, inclusive playable region, add-root key offset, group display names, sample offset and loop crossfade; normalization and silence threshold resolve against decoded PCM; swap/DC/polarity become private effects. Synthetic PCM gives gain2 and window .4..0.8; JSON/history retain the result. |
| Shift / gate / LFO | Published shift0/1024 endpoints -> 0/960 ticks; gate450/1446 -> 3 source ticks/four quarters, 1447 off; full_porta bypass. Public LFO32950 sixteen-step anchor -> BPM/240 Hz, including volume. SCOPE: no interior nonlinear curves in checked sources; every non-anchor value receives a named evidence warning. |
| Channel fields without units/kernel | MIDI-through, declick and legacy trim are individually named; DawVert float drum-stretch tone/over/noise/band/time lacks DSP units; formant offset is decoded but target has only preserve-formants boolean. SCOPE warnings distinguish serialization knowledge from missing operation/units. |
| Note edit groups | Additive/default-empty Project.noteEditGroups, append-only checked command/undo/patch; live grouped selection/move/resize independent of MIDI color, dead IDs inert; byte-stream + history/JSON and UI movement fixtures. |
| Slide targeting/timing | Highest sounding same-color pitch at onset anchors chord transposition, including a slide already in progress; note endings unchanged. Instrument and sampler fixed-buffer numerical tests and independent note-stream fixture. See public Piano roll manual attribution in NOTICE. |
| Controller links | Source/location/flags/smoothing decoded at public offsets; automation clip sources translated. SCOPE per link: formula serialization and smoothing time units absent from DawVert auto.py/PyFLP controller.py/LMMS. MIDI event226 is GreedyBytes without a semantic source layout. No formula or milliseconds invented. |
| Start / punch | Append-only Start/Punch marker variants persist, undo and edit through existing timeline UI. Start anchors default song play, explicit seeks/selected regions win; paired punch defines actual bounded desktop recording region and restores prior transport. Native PCM/desktop capture and handwritten mock-transport fixtures. |
| Song loop / pauses / edges | Positive song-loop extent uses the active arrangement's clips; pause after preceding song loop keeps its label without transport stop; unpaired punch/empty loop produce named warnings without fabricated endpoints; suffix punch and ordinary playback through punch tested. |

### Source-scope evidence and validation status

SCOPE is a source limit, not a claim of unimplemented documented layouts.
Native devices without a usable layout are reported individually by device name:
DawVert definitions/converters, PyFLP plugin parsers and LMMS were checked. The
anonymous Sytrus/Harmless dataset controls do not repair their missing byte layout.
Slicex second-deck/custom region state after the public WAV payload, unknown SSLF
sections and unfinished-definition tails remain opaque with specific evidence.
Unsupported stand-in topology/controls are named separately from format absence;
none is silently replaced with a claimed exact match.

Source bounds and audio budgets may refuse oversized patches, sample layers,
unsupported compressed WAV, more than 128 slices or unavailable references;
review warnings identify those limits. Imported source bytes remain bounded by
the existing source/archive budgets. No new dependencies were added. New source
modules are registered in convert/mod.rs; Cargo automatically discovers
`tests/round6_native.rs`; native slide and existing engine/desktop/frontend test
modules are registered already. Only single-file rustfmt checks and git diff
whitespace checks were performed. Integration must regenerate bindings and
WASM/simulator artifacts and run the existing strict gates before this row can
be accepted. No previous test or tolerance was weakened; event83's fixture now
asserts the correct resonance identity supported by all three public sources.

## 2026-10-13 round seven: decoded fields and stand-in controls (built and tested)

The importer used to say "no corresponding stand-in control" for every decoded
field whose layout entry had no target. Round seven enumerated all of them
(42 native devices have a public layout in the field table; SimSynth and 3x Osc are decoded separately), gave the two devices named in the
round-six table the controls their fields need, and made every other field say
which of four things it is. `convert/native.rs::unmapped` holds the rule and
the test `open_list::decoded_fields_without_a_stand_in_control_are_exactly_the_recorded_list`
pins the last group, so a field leaves it only on purpose. Evidence and
measurements are in `docs/qa/2026-10-13-r7-builder.md`.

### Given a stand-in control in round seven

| Device and field | Stand-in control | Value scaling | Source of the rule |
|---|---|---|---|
| Multiband Delay `bank_1..8/delay.0..15` (128 fields) | Band Delay `banks.N.timeMs.M` | raw float taken as milliseconds, clamped 1..1000 | DawVert `fl_studio.ddef` names the arrays and gives no unit |
| Multiband Delay `bank_1..8/vol.0..15` (128) | `banks.N.level.M` | raw float taken as gain, clamped 0..1 | same |
| Multiband Delay `bank_1..8/pan.0..15` (128) | `banks.N.pan.M` | raw float taken as pan, clamped -1..1 | same |
| Multiband Delay (no field) | `layout` = sixteen bands, `bank` = 1 | the public layout names no playing bank, band frequency, feedback or wet level: bank 1 plays, the other seven are selectable, band edges are Windfall's own (40 Hz to 16 kHz in equal ratios), feedback and mix stay at the stand-in defaults | stated in the import report |
| SimSynth `osc1..3_pw` | Three oscillator synth `oscillators.N.pulseWidth`; a square becomes the pulse waveform | share high = `0.5 - asin((pw - 0.5) * 2) / pi`; other shapes carry it unheard | DawVert `objects/convproj/wave.py` `wave_squ`, `plugconv_ext/n_flstudio_foss.py` |
| SimSynth `osc1..3_o1`, `osc1..3_o2` | `oscillators.N.octaveUp`, `.twoOctavesUp` | the same wave at 2x and 4x pitch, each mixed half and half with what is there | `n_flstudio_foss.py` `add_wave(shape, pw, 2, 0.5)` and `add_wave(shape, pw, 4, 0.5)` |
| SimSynth `svf_band`, `svf_high` | `bandMix`, `highMix` | 0..1 taken as the level of that filter output beside the low-pass; 0 when the source filter is off | named in `fl_studio.ddef`; no public rule |
| SimSynth `lfo_del` | channel LFO start delay (`TargetLfo.delayMs`) on the cutoff and pitch LFOs | `4 * v^3` seconds, the time rule the device publishes for its envelopes | `flp_dec_plugins.py` `simsynth_time`; the converter reads `lfo_del` without a rule of its own |
| SimSynth `lfo_retrigger` | channel LFO `retrigger` | flag | `n_flstudio_foss.py` |
| Channel envelope/LFO flag bit 5 | channel LFO `retrigger` | flag | PyFLP `_EnvLFOFlags.LFOPhaseRetrig` |

### Translated by a rule outside the field table (the report now says which)

`Fruity DX10` `mod_fine`, `mod2_fine` (folded into the operator ratio),
`octave` (carrier ratio), `vibrato`, `lforate` (channel pitch LFO);
`Fruit Kick` / `Fruity Kick` `max_freq` (pitch drop, with `min_freq`);
`Fruity Slicer` `beats`, `bpm`, `attack`, `pitch` and `Slicex` `master_pitch`
(slice reader); `Fruity Center` `on`, `Spreader` `enabled`, `Transient
Processor` `effect_on`, `Effector` `bypass` (the effect slot's enable);
`Fruity Mute 2` `mute`, `channel`; `Fruity Phase Inverter` `state`;
`Fruity Flanger` `inv_feedback`; `Frequency Shifter` `freqtype`;
`Fruity Delay 3` `tempo_sync`.

### Not a sound control (kept in the saved original state; nothing to add)

- Layout structure: every `version*`, `reserved*` and `header` word, and
  Slicex `data_exists` and `sample_header`.
- Editor display or transport state: every `visual_*` and `display_*` field
  (Fruity Limiter, Fruity Multiband Compressor, Transient Processor, Pitch
  Shifter, Tuner), Spreader `theme`, Slicex `play_pause` and
  `live_selection`, Tuner `reactivity` and `bass` (the tuner only measures).
- Named in the public layout without a meaning: every `unknown_*` and `unk_*`
  word (Fruity Limiter, Fruity Blood Overdrive, Fruity Multiband Compressor,
  Fruity Delay 3, SimSynth) and Effector `effect`, whose selector order no
  public source gives.

### Omitted: decoded sound controls with no stand-in control (round seven; superseded by the round-eight list below)

The end state of the import row (`docs/qa/README.md`, decisions of
2026-10-13) is that a described field whose stand-in has no matching control
is named in the import review as omitted, with the original state kept, and
that the list is pinned by a test. Adding these controls to the stand-ins is
work on the stand-ins, not on the importer. Each line below is filed in the
review under "dropped" and ends "omitted: decoded public field with no
corresponding stand-in control (state retained)."

The review also shows them by device. A category keeps at most 200 lines, so
in a project with many devices the lines alone could cut omitted fields
short; `ImportReport::omitted_controls` lists every device with its omitted
field names and how many times the project uses it, and the review shows that
list under "Omitted controls" (`features/flp-import/import-dialog.tsx`).

233 control families on 33 devices. A family written `name.#` or
`bank_#_e/name` stands for each index of that control (every vocoder band, the 12
pitcher keys, the 8 delay-bank units). `fruit kick` and `fruity kick` are two
names of one device.

| Device | Count | Fields |
|---|---|---|
| bassdrum | 17 | `attack`, `butt`, `click_cut`, `click_freq`, `drive`, `noise_a`, `noise_d`, `noise_filter`, `noise_mix`, `noise_pitch`, `noise_s`, `noise_start`, `peak`, `var_base`, `var_peak`, `var_phase`, `var_slide_time` |
| effector | 7 | `input_level`, `mod_rate`, `mod_shape`, `output_gain`, `tempo`, `x_mod`, `y_mod` |
| frequency shifter | 8 | `feedback`, `lr_phase`, `mixer_track`, `shape_left`, `shape_right`, `start_phase`, `stereo`, `type` |
| fruit kick | 1 | `osc_dist` |
| fruity chorus | 6 | `crosscutoff`, `crosstype`, `lfo1_wave`, `lfo2_wave`, `lfo3_wave`, `wetonly` |
| fruity delay 3 | 16 | `delay_type`, `diffusion_level`, `diffusion_spread`, `dry`, `feedback_bits`, `feedback_reso`, `feedback_sample_rate`, `feedbackdist_knee`, `feedbackdist_symmetry`, `feedbackdist_type`, `keep_pitch`, `mod_cutoff`, `smoothing`, `stereo`, `tone`, `wet_input` |
| fruity delay bank | 4 | `2x_oversampling`, `bank_#_e/feedback_leave_first_echo`, `bank_#_e/grain_div`, `bank_#_e/grain_sh` |
| fruity dx10 | 4 | `mod2_velsen`, `mod_thru`, `mod_velsen`, `waveform` |
| fruity fast dist | 2 | `mix`, `type` |
| fruity flanger | 2 | `cross`, `dry` |
| fruity flangus | 2 | `cross`, `dry` |
| fruity free filter | 1 | `center` |
| fruity granulizer | 10 | `depth`, `hold`, `hold_2`, `key_to`, `loop`, `pan`, `speed`, `trans_hold`, `trans_mode`, `wave_spacing` |
| fruity kick | 1 | `osc_dist` |
| fruity limiter | 28 | `comp_att_curve`, `comp_rel`, `comp_sus`, `limiter_att_curve`, `limiter_rel_curve`, `limiter_safety`, `limiter_sus`, `noise_gain`, `s_comp_att`, `s_comp_att_curve`, `s_comp_knee`, `s_comp_ratio`, `s_comp_rel`, `s_comp_sus`, `s_comp_thres`, `s_gain`, `s_limiter_att`, `s_limiter_att_curve`, `s_limiter_ceil`, `s_limiter_rel`, `s_limiter_rel_curve`, `s_limiter_sus`, `s_noise_gain`, `s_noise_rel`, `s_noise_thres`, `s_sat`, `sat`, `sidechain_number` |
| fruity multiband compressor | 11 | `filter_type`, `gain`, `hi_knee`, `hi_state`, `limiter`, `low_knee`, `low_state`, `mid_freq_h`, `mid_freq_l`, `mid_knee`, `mid_state` |
| fruity panomatic | 1 | `lfo_target` |
| fruity phaser | 3 | `freq_range`, `gain`, `num_stages` |
| fruity slicer | 2 | `fitlen`, `stretchtype` |
| fruity soft clipper | 1 | `postgain` |
| fruity stereo enhancer | 2 | `prepost`, `vol` |
| fruity stereo shaper | 4 | `delay`, `dephase`, `iodiff`, `prepost` |
| fruity vocoder | 12 | `bands.#`, `filter`, `freq_bandwidth`, `freq_invert`, `freq_max`, `freq_min`, `freq_scale`, `hold`, `left_right`, `mix_car`, `mix_mod`, `num_bands` |
| fruity waveshaper | 1 | `removedc` |
| pitch shifter | 11 | `channel`, `delay`, `delay_mode`, `density`, `duration`, `feedback`, `formant_correction`, `jitter`, `pitch_mode`, `rand`, `wet` |
| pitcher | 19 | `bypass.#`, `bypass_all`, `finetune`, `formant`, `gender`, `harmonize`, `harmonize_vel`, `key_on.#`, `min_freq`, `octaves`, `ref_chan`, `selector_key`, `selector_type`, `stereo`, `use_midi`, `voice1_pan`, `voice2_pan`, `voice3_pan`, `voice4_pan` |
| plucked! | 3 | `norm_decay`, `release`, `wide` |
| slicex | 6 | `crossfade`, `layering`, `master_lfo_level`, `master_random_level`, `mod_x`, `mod_y` |
| soundgoodizer | 1 | `mode` |
| spreader | 4 | `low_bypass`, `mono`, `postfx`, `prefx` |
| transient processor | 5 | `attack_shape`, `gain`, `release_shape`, `split_balance`, `split_freq` |
| vintage chorus | 11 | `delay_feedback`, `delay_hp`, `delay_time2`, `invert_wet`, `mod_phase_start`, `mod_shape_sine`, `mod_tempo_sync`, `mono_input`, `noise`, `noise_gain`, `noise_gate` |
| vintage phaser | 27 | `color_freq`, `color_gain`, `delay_feedback`, `delay_hp`, `delay_keeppitch`, `delay_lp`, `delay_mix`, `delay_on`, `delay_stereo`, `delay_tempo`, `delay_time`, `feedback_hp`, `input_hp`, `levels_mono`, `levels_noise`, `levels_noise_gate`, `lfo_pull`, `lfo_scale`, `mod_phase_start`, `mod_shape_sine`, `mod_tempo`, `mod_tension`, `phaser_freq`, `phaser_hq`, `phaser_manual`, `rate_freq1`, `rate_freq2` |

Outside the field tables, reported per channel: SimSynth `osc1..3_warm`
(the public converter maps it to a six-voice unison), `lfo_trackamp` and
`chorus_on`; and the channel LFO's `predelay` and `attack`, which the public
sources give only as a percentage with no time unit, and its tempo-sync flag.

## 2026-10-14 round eight: matching controls carried, every omission pinned (built and tested)

The round-eight audit found decoded fields pinned as omitted although their
stand-in has a matching control, and omissions outside the native table that
were folded into an "approximated" line or missing from the list. The rule
now checked by a test is the one in `docs/qa/README.md` (decisions of
2026-10-13, revised): a described field stays omitted only while the
stand-in has no control that could carry it, judged against the stand-in's
own descriptors.

### Given a stand-in control in round eight

| Device and field | Stand-in control | Value scaling | Source of the rule |
|---|---|---|---|
| Pitch Shifter `wet` | Pitch Shift `mix` | `wet / 10000`, clamped 0..1 | the public definition (marked unfinished) gives no range; 0..10000 is the range the public dataset gives the mix of Frequency Shifter and the amount of Spreader, effects of the same generation |
| Pitch Shifter `formant_correction` | `mode` (Voice when on) | any value but 0 is on | named in the public definition; the stand-in's Voice mode is the formant-preserving one |
| Vintage Phaser `color_freq` | Stage Phaser `colorHz` | linear fit of 0..650000 to 200..12000 Hz | dataset range, fitted like the sweep's ends |
| Vintage Phaser `mod_tempo` | `sync`, with `cycleBeats` = import tempo / 60 / fitted rate (1/8..16) | any value but 0 is on | the dataset names the switch; the synced cycle plays the fitted rate at the import tempo |
| Vintage Chorus `mod_tempo_sync` | Bucket Chorus `sync`, `cycleBeats` as above | any value but 0 is on | same |
| Fruity Chorus `wetonly` | Chorus `mix` = 1 (wet only) above 512, the stand-in's 0.5 below | 0..1024, number 11 (its automation follows the same rule) | dataset "Wet Only" |
| Fruity Stereo Shaper `delay` | Stereo Matrix `rightDelayMs` (positive) or `leftDelayMs` (negative) | abs(delay) / 4096 of 50 ms | dataset range -4096..4096, no unit or side; `delay` and `dephase` are now read signed |
| Fruity PanOMatic `lfo_target` | which depth `lfo_amount` drives: 0 `depth` (pan), 1 `volumeDepth`, 2 both | the dataset names an "LFO Target" of 0..2 and none of its values; these are the three ways one amount can drive the stand-in's two depths, 0 being how the amount was read before; automation of the amount follows the target (pan only for "both") | dataset |
| BassDrum `peak` | Layer Kick `pitchDropSemitones` | `12 log2(peak / base)`, 0..60, only when both are positive | `peak` and `base` read in hertz, as `base` has been since round six |
| Fruity Vocoder `num_bands` | Vocoder `bandCount` | clamped to the stand-in's 16..32 | the band count of the public variable record |
| Pitcher `key_on.0..11` | Pitch Correct `root` and `scale` | when the enabled pitch classes (C first) are exactly the chromatic, major, minor or pentatonic scale on some root (first scale, then lowest root); otherwise the stand-in stays chromatic and the review says so | public layout; the stand-in's scales |
| Fruity Reeverb 2 `predelay` | Reverb `preDelayMs` | linear fit of 0..384 to 0..250 ms | dataset range (DawVert divides by 38.4 into a unit it does not state) |
| Fruity Reeverb 2 `mod` | `modulation` | `mod / 2048` | dataset range, "Mod Depth" 0..2048 |
| Fruity Reeverb `predelay` | `preDelayMs` | taken in milliseconds (0..250) | dataset range; DawVert carries it over unchanged |
| Fruity Delay 2 `time_stereo_offset` | Delay `stereoOffsetMs` | linear fit of -512..512 to -50..50 ms | dataset range (DawVert divides by 512) |
| Fruity Delay 2 `fb_cut` | `highCutHz` | linear fit of 0..128 to 500..20000 Hz | dataset range; the same disclosed fit as Fruity Delay 3's `feedback_cutoff` |
| Fruity Delay `cutoff` | `highCutHz` | linear fit of 0..1024 to 500..20000 Hz | same |
| Fruity Delay `tempo` | `timeMs` (not synced) | with the knob up, the steps are counted at its own tempo, `60 + 220 * tempo / 1024` BPM | DawVert `universal__n_flstudio.py` (`set_steps_nonsync`) |

Tests: `crates/windfall-flp/tests/round8_controls.rs` (per device: the
imported value, the audible effect against a reference worked out in the
test, and save and reopen).

### Omitted fields outside the native table, now filed as omitted

Each is a "dropped" line of its category, counted as omitted and listed by
device under "Omitted controls"; none is part of an approximated line any
more. SimSynth `osc1..3_warm`, `lfo_trackamp`, `chorus_on`; 3x Osc
`osc1..3_ofs`, `osc1..3_detune`, `phase_rand`, `osc1..3_invert`, `osc3_am`;
Fruity Reeverb 2 `bass`, `cross`, `stereo`, `mod_speed`, `tempo_predelay`,
`mid_side`; Fruity Reeverb `color`; Fruity Delay 2 `input_pan`; Fruity
Parametric EQ 2 `1..7_order`; and on a channel, under "Channel envelope",
"Channel LFO", "Channel arpeggiator", "Channel note echo" and "Channel time
stretch": envelope `predelay`, `hold`, `attack_tension`, `decay_tension`,
`release_tension`, `tempo_sync`; LFO `predelay`, `attack`, `tempo_sync`;
arpeggiator `chord`, `repeat`, `slide`; note echo `ping_pong`, `fat_mode`,
`pan`; time stretch `formant`. The channel's fields are filed when they are
in use (a predelay of 100 is none), the device fields whenever the device is.

### The pinned list

`convert/native.rs`, module `open_list`:

- `decoded_fields_without_a_stand_in_control_are_exactly_the_recorded_list`
  pins every omitted field, native and channel alike, each with a reason.
- `every_omitted_field_was_checked_against_the_controls_of_its_stand_in`
  works out, from each table device's stand-in descriptors, the controls none
  of its decoded fields sets, and requires them to equal a reviewed line
  saying why no omitted field fits them; outside the table each reviewed
  control must be one the stand-in has, and the channel's own stand-ins are
  destructured field by field, so a field added to them does not compile
  until it is reviewed.
- `devices_outside_the_table_report_exactly_their_recorded_omissions`.

269 omitted families on 43 devices: 222 on 32 devices of the native table
and 47 on 11 elsewhere. A family written `name.#` or `bank_#_e/name` stands
for each index of that control. `fruit kick` and `fruity kick` are two names
of one device. The reason for each row is in the test.

| Device | Count | Fields |
|---|---|---|
| bassdrum | 16 | `attack`, `butt`, `click_cut`, `click_freq`, `drive`, `noise_a`, `noise_d`, `noise_filter`, `noise_mix`, `noise_pitch`, `noise_s`, `noise_start`, `var_base`, `var_peak`, `var_phase`, `var_slide_time` |
| effector | 7 | `input_level`, `mod_rate`, `mod_shape`, `output_gain`, `tempo`, `x_mod`, `y_mod` |
| frequency shifter | 8 | `feedback`, `lr_phase`, `mixer_track`, `shape_left`, `shape_right`, `start_phase`, `stereo`, `type` |
| fruit kick | 1 | `osc_dist` |
| fruity chorus | 5 | `crosscutoff`, `crosstype`, `lfo1_wave`, `lfo2_wave`, `lfo3_wave` |
| fruity delay 3 | 16 | `delay_type`, `diffusion_level`, `diffusion_spread`, `dry`, `feedback_bits`, `feedback_reso`, `feedback_sample_rate`, `feedbackdist_knee`, `feedbackdist_symmetry`, `feedbackdist_type`, `keep_pitch`, `mod_cutoff`, `smoothing`, `stereo`, `tone`, `wet_input` |
| fruity delay bank | 4 | `2x_oversampling`, `bank_#_e/feedback_leave_first_echo`, `bank_#_e/grain_div`, `bank_#_e/grain_sh` |
| fruity dx10 | 4 | `mod2_velsen`, `mod_thru`, `mod_velsen`, `waveform` |
| fruity fast dist | 2 | `mix`, `type` |
| fruity flanger | 2 | `cross`, `dry` |
| fruity flangus | 2 | `cross`, `dry` |
| fruity free filter | 1 | `center` |
| fruity granulizer | 10 | `depth`, `hold`, `hold_2`, `key_to`, `loop`, `pan`, `speed`, `trans_hold`, `trans_mode`, `wave_spacing` |
| fruity kick | 1 | `osc_dist` |
| fruity limiter | 28 | `comp_att_curve`, `comp_rel`, `comp_sus`, `limiter_att_curve`, `limiter_rel_curve`, `limiter_safety`, `limiter_sus`, `noise_gain`, `s_comp_att`, `s_comp_att_curve`, `s_comp_knee`, `s_comp_ratio`, `s_comp_rel`, `s_comp_sus`, `s_comp_thres`, `s_gain`, `s_limiter_att`, `s_limiter_att_curve`, `s_limiter_ceil`, `s_limiter_rel`, `s_limiter_rel_curve`, `s_limiter_sus`, `s_noise_gain`, `s_noise_rel`, `s_noise_thres`, `s_sat`, `sat`, `sidechain_number` |
| fruity multiband compressor | 11 | `filter_type`, `gain`, `hi_knee`, `hi_state`, `limiter`, `low_knee`, `low_state`, `mid_freq_h`, `mid_freq_l`, `mid_knee`, `mid_state` |
| fruity phaser | 3 | `freq_range`, `gain`, `num_stages` |
| fruity slicer | 2 | `fitlen`, `stretchtype` |
| fruity soft clipper | 1 | `postgain` |
| fruity stereo enhancer | 2 | `prepost`, `vol` |
| fruity stereo shaper | 3 | `dephase`, `iodiff`, `prepost` |
| fruity vocoder | 11 | `bands.#`, `filter`, `freq_bandwidth`, `freq_invert`, `freq_max`, `freq_min`, `freq_scale`, `hold`, `left_right`, `mix_car`, `mix_mod` |
| fruity waveshaper | 1 | `removedc` |
| pitch shifter | 9 | `channel`, `delay`, `delay_mode`, `density`, `duration`, `feedback`, `jitter`, `pitch_mode`, `rand` |
| pitcher | 18 | `bypass.#`, `bypass_all`, `finetune`, `formant`, `gender`, `harmonize`, `harmonize_vel`, `min_freq`, `octaves`, `ref_chan`, `selector_key`, `selector_type`, `stereo`, `use_midi`, `voice1_pan`, `voice2_pan`, `voice3_pan`, `voice4_pan` |
| plucked! | 3 | `norm_decay`, `release`, `wide` |
| slicex | 6 | `crossfade`, `layering`, `master_lfo_level`, `master_random_level`, `mod_x`, `mod_y` |
| soundgoodizer | 1 | `mode` |
| spreader | 4 | `low_bypass`, `mono`, `postfx`, `prefx` |
| transient processor | 5 | `attack_shape`, `gain`, `release_shape`, `split_balance`, `split_freq` |
| vintage chorus | 10 | `delay_feedback`, `delay_hp`, `delay_time2`, `invert_wet`, `mod_phase_start`, `mod_shape_sine`, `mono_input`, `noise`, `noise_gain`, `noise_gate` |
| vintage phaser | 25 | `color_gain`, `delay_feedback`, `delay_hp`, `delay_keeppitch`, `delay_lp`, `delay_mix`, `delay_on`, `delay_stereo`, `delay_tempo`, `delay_time`, `feedback_hp`, `input_hp`, `levels_mono`, `levels_noise`, `levels_noise_gate`, `lfo_pull`, `lfo_scale`, `mod_phase_start`, `mod_shape_sine`, `mod_tension`, `phaser_freq`, `phaser_hq`, `phaser_manual`, `rate_freq1`, `rate_freq2` |
| simsynth | 5 | `osc1_warm`, `osc2_warm`, `osc3_warm`, `lfo_trackamp`, `chorus_on` |
| 3x osc | 11 | `osc1_ofs`, `osc1_detune`, `osc2_ofs`, `osc2_detune`, `osc3_ofs`, `osc3_detune`, `phase_rand`, `osc1_invert`, `osc2_invert`, `osc3_invert`, `osc3_am` |
| fruity reeverb 2 | 6 | `bass`, `cross`, `stereo`, `mod_speed`, `tempo_predelay`, `mid_side` |
| fruity reeverb | 1 | `color` |
| fruity delay 2 | 1 | `input_pan` |
| fruity parametric eq 2 | 7 | `1_order` to `7_order` |
| channel envelope | 6 | `predelay`, `hold`, `attack_tension`, `decay_tension`, `release_tension`, `tempo_sync` |
| channel lfo | 3 | `predelay`, `attack`, `tempo_sync` |
| channel arpeggiator | 3 | `chord`, `repeat`, `slide` |
| channel note echo | 3 | `ping_pong`, `fat_mode`, `pan` |
| channel time stretch | 1 | `formant` |

Kept omitted although a stand-in control has a similar name, with the
reviewed reason (all in the test): Fruity Delay 3 `delay_type` and `stereo`
(the dataset gives no values, or no width, to set the stand-in's mode from),
Fruity Reeverb 2 `stereo` (no range for a width), Parametric EQ 2 orders (no
slope or meaning of the sign for the cut slopes), 3x Osc stereo detune (the
stand-in's unison detune is for all oscillators together), SimSynth warmth (a
unison in the public converter; the stand-in has none), Pitch Shifter
`pitch_mode` (no values; formant correction sets the mode), Pitcher
`selector_key`/`selector_type` (no value list; the scale comes from
`key_on`), the channel LFO `predelay` (no public time rule for the start
delay), Fruity Delay Bank `grain_div` (no note value for the division),
Fruity Vocoder `left_right` (the stand-in's second signal is the sidechain
key) and Fruity Multiband Compressor `mid_freq_l`/`mid_freq_h` (the bands
meet at the two crossovers, which `low_freq` and `hi_freq` carry).
