# Sample instruments â€” 2026-10-11

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

Editors now include a key/velocity zone map with mapping, layer, loop and envelope
controls; sixteen auditionable pads; a player-only reference-bank loader; SF2
bank/program browsing; generated keyboard switching; and saved speech rendering.
Public engine/history/disk/live/offline and callback-allocation acceptance tests
are written in the dedicated sample-instrument suites. Mounted editor cases cover
zone undo, pad layers, player refusals, SF2 selection and speech capability errors.

Portable peripheral traits, bounded frame scheduling, an own mock protocol and
force-feedback/lighting mapping helpers are added with mock tests. The public
Windows XInput adapter is behind the engine `xinput` feature. These contracts are
not yet registered rack/mixer processors or session-managed devices. No vendor
lighting SDK was added; a separately supplied compatible adapter is still required.

The existing MIDI output worker now exposes a Windows system-General-MIDI alias
only when a named Microsoft software synth is enumerated. The alias resolves
fresh IDs and never falls back to external hardware; it is selectable in the
existing MidiOut workflow. OS synth audio is outside the Windfall mix/offline path.
No full-row claim is made for these peripheral or system-MIDI paths.
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

## Additional written acceptance suites

All are written, not executed:

- `crates/windfall-dsp/tests/dsp/sample_bank.rs`: callback allocation/free guard
  across all six registered kinds and explicit off-audio source-owner destruction.
- `crates/windfall-project/tests/sample_instruments.rs`: history/JSON/clone/legacy
  omission and atomic stale, missing-source, pad-range and player-edit refusals.
- `crates/windfall-engine/tests/sample_instrument_acceptance.rs`: document-to-disk
  history and live/offline PCM equality, generated piano and speech determinism.
- `apps/desktop/src-tauri/src/session/tests/sample_instruments.rs`: saved speech
  WAV/one-step history/reopen and unsupported/stale recipe refusal.
- `apps/desktop/src/features/sample-instruments/editor.test.tsx`: mounted cases
  described above.
- `crates/windfall-engine/src/peripherals.rs`: protocol roundtrip/truncation,
  scheduler bounds/stop, unplug failure and motor note release mocks.
- `crates/windfall-engine/src/midi_hardware/general_midi.rs`: fresh software-synth
  route resolution and no external-device fallback.

Inspection fixes now resolve incoming SF2 references against the new project
folder, refuse unsupported bank channel-preset export, clear incompatible bank
references on processor changes (undo restores them), and guard asynchronous speech
patches and editor state by project generation. Native direct zone/pad source import
decodes off-guard, reuses existing source IDs, and adds source plus mapping as one
stale-guarded history step without creating an unrelated sampler channel. Added
written tests cover duplicate-source import/undo, replacement during speech work
and temporary-file cleanup, preset kind-change history, and sampled octave pitch
measured by independent zero crossings at four output rates. All nine rows remain partial
until those gaps, held-voice bank replacement, portable bank loading, broader SF2
interoperability and registered peripheral workflows are closed and verified.

## Portable banks and pad sequencing

Written, not executed: own `.wfbank` version-1 JSON manifests contain relative
WAV references and validated zone mappings. Native loading bounds manifests,
rejects traversal/duplicate/missing IDs, decodes sources on the worker, remaps
bank IDs into project SampleIds and attaches the entire instrument in one guarded
history step. Export writes Float32 source copies plus an atomic manifest; failed
exports clean newly written files. The multisampler, drum bank and player editors
can load/save these banks. The player keeps mappings read-only.

The selected drum pad now has its own paged pattern step editor (up to the current
pattern length); toggling a pad changes only notes for that key, preserving other
pads and supporting ordinary note history. Added written mounted case:
`sequences the selected pad without replacing another pad and restores the step with undo`.
Added native case:
`portable_bank_copies_own_pcm_loads_into_a_fresh_player_and_preserves_one_step_history`.
Added format case:
`manifest_rejects_wrong_version_traversal_duplicate_ids_and_missing_sources`.

Portable loading is no longer a project-ID-only JSON workflow. Remaining bank
limits still include active-voice replacement continuity, aggregate retained-plan
memory admission, and broader multisampler modulation/effects. Razer lighting is
still a contract/mock without a supplied SDK, a registered mixer effect or session
relay. XInput is still a feature-gated adapter without a selectable project route.

## Controller feedback project route

The Windows `xinput` feature now exposes `Game controller feedback (XInput)` as a
virtual output in the existing MIDI output selector. Add a MidiOut instrument,
select this route in MIDI settings, and use MIDI output channels 1–4 for controller
indexes 0–3; note keys below 64 drive the low motor, keys 64–127 the high motor,
and velocity sets strength. The existing timestamped MIDI worker owns the adapter,
so notes, panic, disconnect and adapter destruction perform IO away from audio.
No separate RumbleNotes enum or duplicated device-worker lifecycle was introduced.
This reuses the actual registered MidiOut note/automation/persistence path.

Written, not executed:
`worker_output_maps_notes_velocity_channels_panic_and_drop_to_mock_feedback` in
`crates/windfall-engine/src/midi_hardware/feedback.rs`. The desktop feature forwards
to the engine feature. Integrator must also compile/test with `--features xinput`
on Windows and check real controller connect/unplug/panic; no device was accessed
here. A dedicated feedback instrument editor/route picker remains missing, so this
is still a partial claim for `inst-fruity-vibrator`.

## Special output and keyboard editor group

The MidiOut inspector now has dedicated system-General-MIDI and game-controller
feedback route buttons, availability from the actual enumerated outputs, useful
motor/channel instructions, selected output state and explicit native errors.
It refreshes settings before configuration and guards async results by project
generation. These routes deliberately reuse the registered MidiOut instrument;
output selection is the existing app-wide persisted MIDI preference, not an
undoable per-instrument destination. That limitation remains material for the
special rows. The piano editor now exposes the active source recipe, and the zone
editor exposes hold and scale tuning as well as its other envelopes/mappings.

## SF2 multi-preset acceptance group

Written, not executed:
`sf2_selects_only_the_requested_preset_and_resolves_global_instrument_zones`
constructs a real two-preset/two-instrument/two-source SF2 in Rust and checks
preset selection isolation, preset/instrument range intersections, inherited
root tuning, distinct signed PCM, catalog enumeration and missing-program refusal.
This supplements hostile/truncated files, generator offsets, envelopes and loops;
it does not establish broad third-party SF2 interoperability.

## Mounted special-output and source-recipe acceptance group

Written, not executed: `editor.test.tsx` also covers active generated-piano recipe
switching/undo and native portable-bank failure leaving the player unchanged.
`special-output-editor.test.tsx` covers unavailable routes without hardware fallback,
advertised system-synth/controller selection preserving settings, and worker failure.
The native adapters and mock route tests do not fabricate hardware evidence.

## Sample callback cost group

Voice source ratios and pan gains are cached when notes/pitch/pan change. The
brightness filter coefficient is computed once per block, and global transpose
once per frame, rather than repeating exponential/trigonometric work for each
layer on every frame. Saturated pitch bounds remain explicit. The existing
written independent octave-frequency, block-partition equality and callback
allocation tests cover this path; playback CPU still needs integrator measurement.

Source inspection correction: cache initialization uses the selected zone mapping
explicitly. Portable-bank export releases State before atomic file persistence;
its authorized immutable snapshot cannot mutate the document if a subsequent
edit races that final file operation. No tests were run for these changes.
