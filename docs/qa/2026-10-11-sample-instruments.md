# Sample instruments — 2026-10-11

Branch: **gpt/w3-sample-instruments**.
Base: 87b87bc2439ec076d0f75c5b4d997c9db1faba95.
Latest source implementation commit before this hand-off consolidation:
c3a8ad09b59c328e30383100294161b9c60de068.

**All new tests are written, not executed.** The worker ran no builds, tests,
package-manager commands, generators, benchmarks, device checks or sub-agents.
Read/search/edit/git and rustfmt on owned files were used. Git whitespace
inspection and strict UTF-8 reading completed; these do not establish compilation
or acceptance. Generated artifacts and parity status files were not edited.

## Per-row claims

No full-row acceptance is claimed. This replaces the older chronological notes,
including obsolete statements about missing portable banks, pad sequencing,
selectable XInput routes and dedicated output editors.

| Row | Claim | Implemented | Remaining |
| --- | --- | --- | --- |
| inst-directwave-full | Partial | Registered Multisampler, immutable PCM banks, editable key/velocity map, layers/fades/round robin/chokes, tuning, loops/envelopes, source import and portable bank load/save with history. | Broader modulation/internal effects; shared asset gaps below; integrated acceptance. |
| inst-directwave-player | Partial | Registered read-only Sample player, portable banks with source remapping, map display, automatable playback controls and persistence. | Shared asset gaps; missing-source relink/recovery; integrated acceptance. |
| inst-soundfont-player | Partial | Registered SoundFont bank, own checked RIFF/SF2 reader, preset→instrument→zone mapping/browser, loops/envelopes/tuning, saved path/bank/program. | Wider SF2 generators/modulators, stereo/24-bit and delay envelopes; SF2 portability/relink; shared asset gaps and interoperability acceptance. |
| inst-fruity-pad-controller-fpc | Partial | Registered Layered pads, 16 auditionable pads on keys 36–51, layers/chokes/velocity maps, import/portable banks, selected-pad pattern steps preserving other notes and undo. | Click-free chokes and shared asset gaps; integrated acceptance. |
| inst-fl-keys | Partial | Registered Piano keys, own generated struck-string/electric sources, 15 roots × two velocity layers, source switch/undo, cached playback ratios/pan. | Low-CPU measurement/listening, shared asset gaps and integrated acceptance. |
| inst-speech-synthesizer | Partial | Registered Text voice, deterministic own English ASCII rules/formants, independent pitch/duration, worker-rendered saved WAV, one-step attach/history, stale guards and failed-file cleanup. | Native cancellation UI, voice quality review, shared asset gaps and integrated acceptance. |
| inst-fruity-vibrator | Partial | Registered MidiOut drives public Windows XInput through the existing timestamped output worker; four controllers, velocity/motor mapping, note release/panic/disconnect/drop, dedicated availability-aware route button. | App-wide MIDI preference rather than project-owned instrument destination/history; Windows feature/hardware verification. |
| fx-fruity-lsd | Partial | Registered MidiOut selects an enumerated Microsoft OS synth through a stable alias/inspector route; existing bank/program controls, fresh ID resolution, no hardware fallback. | App-wide destination rather than project-owned routing; OS audio outside Windfall mixer/offline export; actual Windows validation. |
| fx-razer-chroma | Partial | Peripheral adapter, bounded own mock protocol/scheduler, tempo/audio-energy→RGB helper, mock stop/failure tests. No vendor SDK dependency. | Registered mixer effect, actual tempo/audio feed/session relay and vendor adapter absent. External SDK/license/distribution and hardware gates; contract alone closes no row. |

## Implementation and ownership

Six kinds and parameter variants were appended: MultiSampler, SamplePlayer,
SoundFont, PadBank, PianoKeys, TextVoice. Their small Copy controls are level,
tune, release and brightness. PCM never belongs to parameters. The older zones/
DSP seam is left intact.

windfall-dsp::sample_bank validates immutable banks: at most 256 zones, 256 MiB
conservatively charged PCM per bank and 128 fixed layer voices. Zones persist
project SampleId references, inclusive key/velocity ranges, independent linear
fades, round-robin/choke groups, pan/gain/tuning, loop frames/mode and volume
envelopes. Layers add; fades are not normalized. Ordinary mono/stereo sample
sources are supported. Missing sources, invalid PCM/loops and unsupported content
return preparation errors.

Callbacks own indexes into a whole processor-held bank; they do not load sources
or destroy per-voice Arcs. Pitch ratios/pan gains are cached on note/control changes,
filter coefficients per block. Inactive slots are ignored by pitch/pan/gain,
including note ID 0 in empty banks. Missed/stolen counters exist on DSP instruments
but are not yet exposed through IPC.

Optional Channel.sampleInstrument has legacy serde defaults. SetSampleInstrumentAsset
and EditSampleZones use expected-value checks and channel history/patches. Players
reject mapping edits; pad zones target one key 36–51. In-use samples cannot be
removed. Incompatible processor changes clear the reference; undo restores it.
Channel-preset export refuses unsupported bank content instead of dropping it.

Engine snapshots prepare banks before plan publication. Compilation only reads
ready handles. Unchanged assets/source identities reuse banks; unchanged bank/kind
reuses the live unit for parameter edits. SF2 incoming references resolve against
the incoming project folder. Existing control/plan ownership retires processors
and sources away from audio.

Native Windfall-authored .wfbank version-1 JSON stores bank-local source IDs,
relative WAV references and zones. Loading bounds manifests, rejects traversal,
duplicate/missing references, decodes off State, remaps IDs and attaches in one
guarded undo step. Export copies Float32 WAV sources into an adjacent folder and
atomically persists the manifest outside State. Failed work removes new copies.
A later edit cannot mutate the exported immutable snapshot's document. Superseded
successful copies are not garbage-collected.

Direct import reuses source IDs and adds source plus mapping in one history step,
without an unrelated sampler channel. Speech renders away from audio/State/
recording guards, saves an app-owned WAV, then checks generation/edit/recipe/ticket
before attachment. Failed/stale work cleans its file; successful files survive undo.

Frontend editors expose zones/loops/envelopes, layers, 16 pads, selected-pad
pattern pages, portable banks, SF2 presets, keyboard switching and speech save.
Views reset on project generation. Reopened SoundFont editors restore saved
external paths. Native capabilities/errors are explicit; browser mode does not
fabricate file rendering or hardware output.

## Provenance and dependency decisions

sample_bank/sources.rs generates all keyboard sources with newly authored struck
partials, slight inharmonicity and per-partial decay, including the electric recipe.
Speech uses newly authored grapheme/digraph rules, pulse/noise excitation and
formant resonators. It is robotic English ASCII, not a proprietary/recorded voice.
Recipes are version 1. No proprietary samples, kits, presets, voices or new
dependencies were introduced.

The own SF2 reader validates RIFF bounds/padding, table widths, hydra references,
generator ranges and sample/loop addresses. It supports mono 16-bit PCM,
preset/instrument global/local zones, range intersections, address offsets,
root/coarse/fine/scale tuning, exclusive groups and volume envelopes/preset offsets.
It rejects nontrivial modulators, unsupported filter/LFO generators, sm24,
stereo types and positive delay-volume envelopes. This is a documented subset;
third-party SF2 interoperability is not established.

The opt-in Windows xinput engine feature is forwarded by desktop. Public
XInputSetState from Xinput9_1_0 runs solely on a worker, including panic/destruction.
MIDI channels 1–4 select controllers 0–3; keys below 64 drive low motors, 64–127
high motors; velocity controls held strength.
[Public Microsoft API](https://learn.microsoft.com/en-us/windows/win32/api/xinput/nf-xinput-xinputsetstate).

The system synth alias is advertised only for enumerated Microsoft GS Wavetable
Synth names and reuses the native MIDI worker/MidiOut controls. No device was accessed.

No Razer vendor SDK was added/bundled. Its GPL/distribution compatibility remains
an external dependency decision per the task; a separately supplied adapter is
not presumed available. The RGB helper/mock is not a hardware feed.

## Tests — all written, not executed

crates/windfall-dsp/src/sample_bank/tests.rs:

- inclusive_key_velocity_edges_and_independent_fades
- empty_bank_and_retired_note_controls_are_safe_and_silent
- overlapping_layers_add_and_missing_keys_are_silent
- round_robin_advances_only_matching_groups_and_chokes_all_old_layers
- instances_release_independently_and_stop_retires_bounded_voices
- live_style_partitions_match_offline_pcm_with_loops_and_envelopes
- invalid_pcm_mapping_and_loops_fail_before_publication
- speech_render_is_deterministic_bounded_and_cancellable
- generated_piano_has_multiple_keys_and_velocity_sources
- sf2_real_pcm_and_preset_ranges_are_not_limited_to_1024_frames
- sf2_every_truncated_prefix_and_forged_link_fails_without_panicking
- sf2_envelopes_tuning_and_preset_offsets_reach_resolved_zone
- sampled_pitch_matches_independent_frequency_measurement_at_multiple_rates
- sf2_selects_only_the_requested_preset_and_resolves_global_instrument_zones

Fixtures construct real-sized SF2 PCM and two presets/instruments/distinct sources
in Rust. Truncated prefixes/forged links exercise rejection. Independent
zero-crossing measurements check pitch at multiple output rates.

crates/windfall-dsp/tests/dsp/sample_bank.rs (registered in main.rs):

- asset_instrument_callbacks_never_allocate_or_free_and_sources_retire_off_audio

crates/windfall-project/tests/sample_instruments.rs:

- bank_references_survive_history_json_clone_and_legacy_omission
- stale_missing_invalid_pad_and_player_mapping_edits_are_atomic
- incompatible_instrument_changes_clear_bank_references_and_undo_restores_them

crates/windfall-engine/tests/sample_instrument_acceptance.rs:

- mapped_layers_reach_history_disk_and_equal_live_offline_pcm
- own_piano_and_speech_recipes_are_public_instruments_with_deterministic_pcm

These exercise documents/pools/plans and uneven live/offline block sizes.

apps/desktop/src-tauri/src/session/tests/sample_instruments.rs (registered):

- speech_render_saves_a_source_as_one_history_step_and_reopens
- unsupported_speech_and_stale_recipe_do_not_change_document_or_history
- import_zone_reuses_sources_and_undo_removes_the_layer_without_extra_channels
- replacement_during_speech_render_refuses_commit_and_cleans_the_saved_source
- portable_bank_copies_own_pcm_loads_into_a_fresh_player_and_preserves_one_step_history

session/sample_bank_files.rs:

- manifest_rejects_wrong_version_traversal_duplicate_ids_and_missing_sources

crates/windfall-engine/src/peripherals.rs:

- mock_protocol_roundtrips_and_rejects_hostile_bytes
- mock_scheduler_waits_orders_bounds_and_panic_discards_old_commands
- mock_disconnect_stops_and_notes_release_only_their_motor

midi_hardware/general_midi.rs:

- system_route_resolves_fresh_ids_and_never_selects_external_hardware

midi_hardware/feedback.rs:

- worker_output_maps_notes_velocity_channels_panic_and_drop_to_mock_feedback

apps/desktop/src/features/sample-instruments/editor.test.tsx:

- maps a source into a zone, edits a boundary and restores it through undo
- shows sixteen pads and adds independent layered sources to the selected pad
- sequences the selected pad without replacing another pad and restores the step with undo
- keeps the player mapping read-only and reports malformed banks
- browses bank/program presets and retains their file reference
- reopens the saved SoundFont path and browses that bank
- switches generated piano recipes with undo and exposes the active source
- reports a native portable-bank loading error without changing the player
- shows desktop-only speech capability without fabricating a rendered source

special-output-editor.test.tsx:

- shows unavailable routes without selecting an external device
- selects an advertised special route while preserving current settings
- reports worker route failure

Existing DSP parameter pins, frontend exhaustive access and sim EVERY_COMMAND
examples also gained the new kinds/commands.

## Integrator commands — not run by worker

After seam reconciliation, from the integrated checkout in Git Bash with the
established MSVC and Node/pnpm environment:

~~~bash
source scripts/msvc-env.sh
task_bindings="$(mktemp -d)"
export TS_RS_EXPORT_DIR="$(cygpath -w "$task_bindings")"

cargo test -p windfall-dsp --lib sample_bank
cargo test -p windfall-dsp --test dsp sample_bank
cargo test -p windfall-dsp --test dsp params
cargo test -p windfall-project --test sample_instruments
cargo test -p windfall-engine --test sample_instrument_acceptance
cargo test -p windfall-engine --lib peripherals
cargo test -p windfall-engine --lib midi_hardware
cargo test -p windfall-desktop --lib sample_instruments
cargo test -p windfall-desktop --lib sample_bank_files

# Windows public adapter compile/harness; real hardware is a separate gate.
cargo test -p windfall-engine --lib --features xinput midi_hardware
cargo test -p windfall-desktop --lib --features xinput sample_instruments

cargo clippy -p windfall-dsp -p windfall-project -p windfall-engine -p windfall-desktop --all-targets -- -D warnings
cargo clippy -p windfall-engine -p windfall-desktop --all-targets --features xinput -- -D warnings

scripts/gen-bindings.sh
scripts/build-sim.sh
node scripts/check-bindings.mjs
node scripts/check-sim.mjs
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop test src/features/sample-instruments src/features/channel-rack/inspector/instrument-section.test.tsx src/features/params/access.test.ts src/lib/ipc/sim/document.test.ts --maxWorkers=4
pnpm --dir apps/desktop exec eslint src/features/sample-instruments src/features/channel-rack/inspector/instrument-section.tsx src/lib/ipc/backend.ts src/lib/ipc/tauri.ts
~~~

Regenerate before mounted/sim tests: checked-in WASM lacks the new commands/kinds.
Complete broader integration checks after targeted failures are resolved.

## Shared seams and regeneration

Functional/shared seams touched:

- DSP: src/{instrument,lib}.rs, tests/dsp/{main,params}.rs.
- Project: src/{model,command,lower,check,lib,preset}.rs; layer.rs has a Channel literal.
- Engine: src/{lib,pool,plan,state,rack,sampler_processing}.rs,
  src/midi_hardware/{mod,native,tests}.rs and Cargo.toml.
- Native: src/commands.rs, src/session/{mod,preparation,sampler_processing,files}.rs,
  src/session/tests/mod.rs and desktop Cargo.toml.
- Frontend: features/channel-rack/inspector/instrument-section.tsx,
  features/params/access.test.ts, lib/ipc/{backend,tauri}.ts,
  lib/ipc/sim/document.test.ts.

New modules: DSP sample_bank/, project sample_instrument.rs, engine
sample_instruments.rs/peripherals.rs/midi_hardware/{feedback,general_midi}.rs,
native session/{sample_instruments,sample_bank_files}.rs, frontend
features/sample-instruments/ and the dedicated acceptance suites above.

Additional narrow Channel-literal changes: engine
src/{controller,project_preparation,qa_mixer_pdc_ports,sequencer,stems}.rs,
src/bin/windfall-soak.rs, tests/engine/support.rs,
tests/{phase_meter,playlist_bounce,playlist_track_solo,spectrogram_view,spectrum_view}.rs.
Get the complete inventory with:
~~~bash
git diff --name-only 87b87bc2439ec076d0f75c5b4d997c9db1faba95..gpt/w3-sample-instruments
~~~

Regenerate TS bindings, instrument descriptor catalog, sim WASM and metadata.
Changed types: Channel, Command, InstrumentKind, InstrumentParams.
New types: SampleInstrumentAsset, SampleZone, BankLoop, SoundFontPreset,
MultiSamplerParams, SamplePlayerParams, SoundFontParams, PadBankParams,
PianoKeysParams, TextVoiceParams.
Commands: SetSampleInstrumentAsset and EditSampleZones.
This branch has 33 instrument kinds (base 27 + six). Reconcile append-only with
parallel streams, every exhaustive match/ALL list and descriptor index order.
No generated artifacts or parity tracker changes were committed.

## Risks and remaining gates, in check order

1. Nothing was compiled. Reconcile enum/descriptor/Channel seams, regenerate, then
   compile Rust/TypeScript. Least-sure spots: cached-voice disjoint borrows,
   native preparation tickets, optional backend/Base UI types and strict Clippy.
   Check downstream exhaustive SamplerPreparationError::Asset matches.
2. Replacing a bank builds a fresh unit and drops held playback instead of carrying
   voices into a new generation. Parameter edits retain the unit. Chokes hard-stop
   layers and can click. The 256 MiB candidate-project bound does not cap all
   retained old plans or concurrent preparation workspaces. These shared asset
   gaps are material and are not hidden test results.
3. Verify SF2 interoperability with own/licensed files: defaults, modulators,
   stereo linking, envelopes/tuning. Missing SF2 sources can refuse preparation;
   silent recoverable instrument/relink UX and SF2 archive support are absent.
4. Run native stale/undo/disk and mounted tests against fresh WASM. Speech has pure
   renderer cancellation, not an exposed native job button. Portable export keeps
   superseded successful source copies; no bank-source GC exists.
5. Run pitch/live-offline/allocation assertions, then target-machine CPU and musical
   listening. No low-CPU, xrun, voice-quality or sonic-equivalence evidence exists.
6. On Windows with xinput, verify link/FFI and actual connect/unplug/panic; check OS
   synth enumeration/output. Neither route has project-owned destination history;
   OS synth PCM does not participate in Windfall offline exports.
7. Lighting is a mock/adapter seam. Registered effect, session feed, compatible
   external vendor integration and actual hardware remain. No SDK redistribution
   or hardware acceptance is claimed.
