# Channel rack, sampler and transport — 2026-10-10

Stream T0. Branch `worktree-agent-acd14ef70e899a998`, based on `fd60cdde`.

**Nothing in this report is a pass record.** Builds and tests were stopped
part-way through this work at the user's instruction. Everything below is
"written, not executed" unless a line says otherwise. The parent builds, runs
and repairs after merging. The two runs that did happen are listed under
"What was executed" and cover an intermediate state only.

No row is claimed done here. Each row lists what was implemented, the tests
written for it, the spots most likely to fail to compile or pass, and what
stays open.

## What was executed, and when

Before the build freeze, on Windows with `CARGO_BUILD_JOBS=4`:

- `cargo check -p windfall-project -p windfall-engine --all-targets`: clean,
  at the state of the first engine and model edits.
- `cargo test -p windfall-engine --test engine rack_voice`: 25 passed, 0
  failed, at the state where held echo, the voice filter, per-target LFOs and
  layers were in the engine.
- `cargo test -p windfall-project -p windfall-engine`: every engine test
  binary passed; the run then stopped at
  `windfall-project --test commands an_instrument_channel_has_this_json`,
  which compares a channel's JSON literally. That failure is why the new
  voice fields are now left out of a file while at their defaults.

Edits made after those runs and never compiled:

- `crates/windfall-project/src/channel_voice.rs`: `skip_serializing_if` and
  `#[ts(as = "Option<..>", optional)]` on `resonance`, `lfos`,
  `filter_settings` and `layer`.
- `crates/windfall-engine/src/sample_preprocess.rs` (new module) and its
  `pub mod` line in `lib.rs`.
- Everything in `crates/windfall-ipc`, `apps/desktop/src-tauri` and
  `apps/desktop/src`.
- `crates/windfall-project/tests/rack_voice.rs` and the project `layer`
  unit tests were never run at all.

## Rows

### `win-rack-echo-delay` — closest to complete

Implemented:

- Echo of notes held by hand (UI keyboard and MIDI hardware), which the audit
  listed as unsupported. Every repeat's note-on is queued at note-on and its
  release is reserved but left open, so the fixed queue's capacity is settled
  then and the later release cannot fail. On key-up each repeat is released
  as long after that moment as it started after the note, so an echo lasts as
  long as the note did (at least one frame).
  `NoteEcho::schedule_held` / `release_held` / `cancel` in
  `crates/windfall-engine/src/channel_voice.rs`; held-echo table in
  `channel_voice_runtime.rs`; `Processor::start_on` / `release_input_on`.
- Echoes now sound alongside the note they repeat. Before, an echo of the
  same key replaced the note through the per-key held table, so a held note
  was cut by its own first echo. Echo voices are owners that are not entered
  in the held table (`Owner::admitted`). The voice limit still counts them
  and they give way first: the oldest echo is released for any new note, and
  a new echo is left out when only played notes are sounding. A one-voice or
  mono-legato channel keeps the old replace behaviour.
- Stop and MIDI panic cut queued and sounding echoes
  (`Processor::cut_echoes_of`). Before, queued echoes kept firing after Stop.
- Refused echoes are observable: `Controller::rejected_echoes()` counts notes
  whose echoes did not fit the 256-slot queue or the 32-entry held table. The
  dry note always sounds.

Tests written, `crates/windfall-engine/tests/engine/rack_voice.rs`
(registered in `tests/engine/main.rs`):
`a_held_live_note_echoes_while_held_and_each_echo_lasts_as_long_as_the_note`,
`held_echo_does_not_depend_on_how_the_output_is_cut_into_blocks`,
`held_echo_repeats_are_transposed_and_fade_by_the_feedback`,
`a_key_released_before_its_first_echo_still_gets_whole_echoes`,
`tempo_synced_held_echo_follows_the_project_tempo`,
`hardware_notes_echo_like_live_notes_and_panic_cuts_their_echoes`,
`stopping_the_transport_cuts_queued_and_sounding_echoes`,
`echoes_that_do_not_fit_are_counted_and_their_notes_stay_dry`,
`sequenced_echo_sounds_the_same_live_and_offline`,
`held_echo_never_allocates_or_frees_on_the_audio_path`.
These ten passed in the one run above.

Remaining:

- `rejected_echoes` is a controller counter only. It is not in an IPC type or
  the desktop UI.
- A seek or loop jump discards the queued echoes of held notes with the rest
  of the echo queue (`ChannelVoiceRuntime::end_sequenced`). Voices already
  sounding are released. Decide whether that is the wanted behaviour.
- While a held note has queued echoes the channel renders in one-frame
  subblocks, as arpeggiated channels already did. Not measured.
- Instrument channels use the same routing but have no test here.
- Listening on real material.

### `win-rack-channel-envelopes`

Summary asks for envelopes and LFOs for volume, pan, pitch and two modulation
targets, plus a filter, on sampler channels.

Implemented:

- Model, `crates/windfall-project/src/channel_voice.rs`: `VoiceFilter`
  (`enabled`, `mode` low-pass/high-pass/band-pass/notch, `cutoff_hz` 20 to
  20000, `resonance` 0 to 1), a `resonance` envelope, `ChannelTargetLfos`
  with one `TargetLfo` each for volume, pan, pitch, cutoff and resonance, and
  `LfoTarget::Volume` / `Resonance` appended for the existing assignable LFO.
  Cutoff is modulation target X and resonance is target Y. All fields have
  serde defaults, sanitize on load and on the command, and are omitted from a
  file at their defaults.
- Engine: `PreparedChannelEnvelopes` runs four envelopes and six LFOs into a
  bounded `ModulationBlock` (adds `resonance` and `volume`). Sampler voices
  (`voice.rs`) apply the filter in its mode with modulated cutoff and Q,
  cache the coefficients while nothing moves, and apply the volume LFO as a
  tremolo that never exceeds unity. With the filter off, behaviour is as
  before.
- Volume envelope: this is the sampler's existing envelope
  (`SamplerSettings.envelope`). It was not duplicated.
- UI, `features/channel-rack/voice/voice-panel.tsx`: Channel filter section,
  Resonance envelope, five target LFO sections, Volume and Resonance as
  assignable-LFO targets. Sampler-only controls are hidden on instrument
  channels. A knob drag is now one undo step (gesture id), and the panel
  follows the saved settings without a remount, which fixes the remount on
  every edit that ended drags.

Tests written:

- Engine `rack_voice.rs`: `the_channel_filter_passes_and_rejects_by_its_mode`,
  `a_filter_that_is_switched_off_changes_nothing_whatever_its_settings`,
  `resonance_raises_the_peak_at_the_cutoff`,
  `the_cutoff_envelope_and_lfo_move_the_first_modulation_target`,
  `the_resonance_envelope_and_lfo_move_the_second_modulation_target`,
  `the_volume_lfo_is_a_tremolo_between_its_depth_and_full_level`,
  `the_pan_lfo_and_envelope_move_the_note_between_the_sides`,
  `the_pitch_envelope_and_lfo_bend_the_note`,
  `modulation_is_the_same_for_every_block_size_and_offline`,
  `modulation_never_allocates_or_frees_on_the_audio_path`. These ten passed
  in the one run above.
- Project `crates/windfall-project/tests/rack_voice.rs`:
  `filter_envelope_and_lfo_settings_are_sanitized_undoable_and_saved`,
  `files_from_before_the_filter_and_layers_load_with_neutral_defaults`.
  Never run.
- UI `voice/voice-panel.test.tsx`: five added cases (filter, resonance
  envelope and target LFO drafts; optional settings left out of an edit;
  instrument channels; following saved settings after a save settles;
  sanitizing). `rack-voice.qa.test.tsx`:
  `saves filter, envelope and LFO edits as undo steps the panel follows`.
  Never run.

Remaining:

- Instrument channels get the pan, pitch and cutoff LFOs through the existing
  expression routing. The channel filter, resonance target and volume LFO do
  nothing on them and are hidden. The row's summary says sampler channels.
- The existing limits for instruments stay: cutoff expression is ±4 octaves
  and modulation does not run during an instrument's release tail.
- No UI test drags a knob, so "one drag is one undo step" is untested.
- Listening on real material.

### `inst-channel-sampler`

Summary: one-sample channel with pitch, envelope, loop and stretch settings.
The row's note said the filter and further modulation were unfinished. Those
are the work listed under `win-rack-channel-envelopes`; nothing else was
changed for this row. It depends on that row, on `win-rack-sampler-stretch`
and on `win-rack-sampler-precomputed` passing.

### `win-rack-sampler-precomputed`

Implemented:

- Kernel, `crates/windfall-engine/src/sample_preprocess.rs`: `preprocess`
  renders a new buffer through trim, reverse, DC removal, polarity, channel
  swap, equal-power fades, a crossfaded loop seam and normalize. The seam is
  blended in the order the sampler plays the region. With audio before the
  loop the loop keeps its length; a loop from the first frame is shortened by
  the blend. The loop points and the region are returned for the new buffer.
- Session job, `apps/desktop/src-tauri/src/session/sampler_preprocess.rs`,
  on the audio editor's asset contract: source file never written, new
  Float32 WAV under `recordings/Sample edits`, one batch (`AddSample`,
  `SetChannelSample`, `UpdateSampler`) through `sample_edit_ticket` and
  `finish_sample_edit`, so a spectral bank is prepared off the locks. A
  result for a changed project, sampler or source is refused and its file
  deleted.
- IPC `SamplerPreprocessRequest`, Tauri command `sampler_preprocess`,
  `Backend.samplerPreprocess`. The browser backend rejects with a reason.
- UI, `inspector/preprocess-section.tsx` and `preprocess.ts`.
- The reverse switch and the playback loop crossfade are unchanged. They are
  still playback settings. A blended seam sets the playback crossfade to 0.

Tests written, none run:

- Engine unit tests in `sample_preprocess.rs` (ten).
- Session `apps/desktop/src-tauri/src/session/tests/sampler_preprocess.rs`
  (registered in `tests/mod.rs`):
  `preprocess_renders_a_new_sample_as_one_undo_step_and_survives_save_and_reopen`,
  `reversing_writes_reversed_audio_and_keeps_the_region_and_loop_on_their_frames`,
  `refused_requests_change_nothing_and_leave_no_file`,
  `a_result_for_a_changed_project_is_dropped_with_its_file`.
- UI `inspector/preprocess.test.ts` (four) and `rack-voice.qa.test.tsx`
  `refuses sample pre-processing in the browser without changing anything`.

Remaining:

- No garbage collection of processed files, as with audio edits.
- Processed samples are external app-owned files, not part of a portable
  bundle until the project is saved with its samples.
- Normalizes to 0 dBFS peak. The audio editor uses −1 dBFS. Pick one.
- No waveform preview of the result before Apply.
- Listening on real loops.

### `win-rack-sampler-stretch`

The prepared independent stretch was already in place. Added the part of the
summary that was missing, "to fit the tempo": a beats field and **Fit to
tempo** in the stretch section. Independent mode sets the duration ratio
draft (then Apply stretch prepares it); tape mode sets the tune in one undo
step. Helpers in `inspector/tempo-fit.ts`.

Tests written, none run: `inspector/tempo-fit.test.ts` (five). No mounted
test clicks the new button.

Remaining: the four review findings in `docs/SAMPLER-REPAIRS.md` were
repaired by their owners before this stream and were not re-verified here;
musical listening; a mounted UI test for Fit to tempo.

### `win-rack-layer-channel`

Implemented:

- Model: `LayerSettings { enabled, children }` on the channel's voice tools,
  with `LayerChildren` a bounded (16), ordered, repeat-free list stored inline
  so the settings stay `Copy`. No new `Channel` field and no new
  `ChannelSource` variant, so no struct literal or match elsewhere changes.
- Rules, `crates/windfall-project/src/layer.rs`: children exist, a layer does
  not play itself, layers do not loop. Checked on
  `SetChannelVoiceSettings` and in `Project::check`. Deleting a channel takes
  it out of every layer in the same undo step.
- Engine: the plan resolves each layer to the channels that finally sound,
  nested layers followed, each channel once, at most 64. The processor fans
  note-on and key-up out to that list. A layer never sounds itself. A muted
  layer passes nothing on. A soloed layer keeps its children audible.
- UI: `channel.addLayer` action (rack menu, toolbar add menu, palette),
  `inspector/layer-section.tsx` with a switch and a checkbox per channel.

Tests written:

- Engine `rack_voice.rs`:
  `a_layer_plays_its_notes_on_every_child_and_makes_no_sound_itself`,
  `nested_layers_reach_each_sounding_channel_once`,
  `live_layer_notes_start_and_release_on_every_child`,
  `mute_and_solo_treat_a_layer_as_the_source_of_its_childrens_notes`,
  `layers_sound_the_same_offline_and_never_allocate_on_the_audio_path`.
  These five passed in the one run above.
- Project `layer.rs` unit tests (five) and `tests/rack_voice.rs`:
  `a_layer_is_set_checked_undone_and_saved_as_one_edit`,
  `deleting_a_child_takes_it_out_of_its_layers_in_the_same_undo_step`,
  `a_duplicated_layer_keeps_its_children_and_a_new_layer_is_one_batch`,
  `damaged_layer_lists_in_a_file_are_refused`. Never run.
- UI `rack-voice.qa.test.tsx`:
  `adds a layer channel, picks its children and keeps them consistent`,
  `keeps a layer's children distinct, ordered and bounded`. Never run.

Remaining:

- The layer's own arpeggiator, echo and polyphony settings are not applied.
  Notes go to the children as played and each child applies its own.
- The layer's per-channel timing (swing, gate, shift) applies; the children's
  timing does not apply to layered notes.
- Soloing a layer also lets its children's own notes through.
- MIDI export, stems and the piano roll treat a layer as an ordinary channel
  with notes. They do not expand it.
- No badge on the rack row marks a layer.

### `wf-transport`

Summary: start and stop playback and recording, and switch between playing
one pattern and the whole song.

No product code was changed for this row. Reading the source and the
existing tests found the summary's behaviour already implemented:
`session/transport.rs` (`transport_play`, `transport_stop`,
`transport_toggle`, `transport_seek`, `transport_set`) and
`session/recording.rs` (`recording_start`, `recording_stop`,
`recording_cancel`), with parts tested in `session/tests/playback.rs`,
`session/tests/recording.rs` and
`features/transport/transport-bar.test.tsx` (`starts and stops playback
through the backend`, `switches between pattern and song`).

What was missing was the row's summary proven as one flow. Written, not
executed, `apps/desktop/src-tauri/src/session/tests/transport_acceptance.rs`
(registered in `tests/mod.rs`):

- `the_transport_plays_stops_records_and_switches_between_pattern_and_song`:
  pattern playback is heard and the position moves; stop silences and
  returns the playhead; a take is recorded from a synthetic input at bar two
  while edits and mode changes are refused; song mode is silent before the
  take, plays it at its tick and reports the song position; pattern mode
  plays the pattern again; every state change is announced in order; the
  take is one undo step.
- `a_cancelled_recording_leaves_the_transport_and_the_project_as_they_were`.

Least sure in that test: the exact list of announced transport states, the
`rms == 0.0` checks after a stop, and the size of the synthetic take (the
sink is fed twice the frames the handle reports, as `recording.rs` does).

Remaining:

- Recording from a real input device, with real latency and drift. The test
  uses a synthetic capture. This needs hardware and stays external.
- The row's note says multitrack workflows remain. `recording_multitrack.rs`
  exists and belongs to the recording rows; it was not assessed here.
- Tap tempo (`docs/TAP-TEMPO.md`) and the toolbar memory readout
  (`docs/TOOLBAR-MEMORY.md`) have their own earlier evidence and were not
  touched.
- No mounted UI test drives record start and stop from the transport bar
  against the session; `recording-dialog.test.tsx` covers the dialog.

## Least sure to compile or pass

Go to these first.

1. `crates/windfall-project/src/channel_voice.rs`: `#[ts(as =
   "Option<LayerSettings>", optional)]` and its three siblings were added
   after the last compile. The same attribute style is used on
   `Channel.timing` in `model.rs`.
2. `apps/desktop/src-tauri/src/session/sampler_preprocess.rs`: never
   compiled. Check the tuple destructuring, `self.inner.cache.decode`,
   `self.pause` under `cfg(test)` and the `SampleEditTicket` calls against
   `playlist_bounce.rs`, which it copies.
3. `crates/windfall-engine/src/sample_preprocess.rs`: never compiled. The
   seam arithmetic for a reversed region and the exact frame ranges asserted
   in its tests are the likeliest test failures.
4. Session test `preprocess_renders_...`: assumes `add_channel_from_file`
   leaves the new channel last in the rack and its sample loaded, and that a
   Float32 WAV decodes bit-exact.
5. Session test `reversing_writes_...`: asserts loop points `(0.6, 1.0)` and
   region `(0.5, 0.75)` as exact `f32` values.
6. TypeScript against bindings that do not exist yet: `VoiceFilter`,
   `VoiceFilterMode`, `TargetLfo`, `ChannelTargetLfos`, `LayerSettings`,
   `SamplerPreprocessRequest`, and `LfoTarget` gaining `"volume"` and
   `"resonance"`. `LayerSettings.children` is expected as `Array<ChannelId>`
   from `#[ts(as = "Vec<crate::ChannelId>")]` on `LayerChildren`.
7. `voice-panel.tsx`: the split into `ChannelVoicePanel` and `VoiceEditor`,
   the context for gesture handlers, and `saved instanceof Promise`. ESLint's
   React hooks rules were not run; `draft.current` is touched in handlers
   only.
8. `rack-voice.qa.test.tsx`: the checkbox accessible names (`Play on Kick`),
   and the loop refusal, which goes through the store's error report.
9. `inspector/preprocess-section.tsx`: `toggle()` returns JSX from a helper
   called during render; the `mounted` ref pattern.
10. Prettier was not run on any TypeScript. Formatting was done by hand.
11. `cargo fmt` was run only on new Rust files (see below). Edited Rust files
    were formatted by hand and may need `cargo fmt`.

## Shared seam files touched

- `crates/windfall-project/src/lib.rs`: `pub mod layer;`.
- `crates/windfall-project/src/check.rs`: one call to `layer::check`.
- `crates/windfall-project/src/lower.rs`: layer check in
  `SetChannelVoiceSettings`; child removal in `remove_channel`.
- `crates/windfall-ipc/src/lib.rs`: `mod sampler_preprocess;` and its
  `pub use`.
- `apps/desktop/src-tauri/src/commands.rs`: `sampler_preprocess` command and
  its handler registration.
- `apps/desktop/src-tauri/src/session/mod.rs`: `mod sampler_preprocess;`.
- `apps/desktop/src-tauri/src/session/tests/mod.rs`: `mod sampler_preprocess;`.
- `apps/desktop/src/lib/ipc/{backend,tauri,mock}.ts`: `samplerPreprocess`.
- `apps/desktop/src/features/channel-rack/actions.ts`: `channel.addLayer`.

Not in the seam list but shared in practice: `crates/windfall-engine/src/`
`lib.rs` (one `pub mod`), `plan.rs`, `processor.rs`, `voice.rs`, `shared.rs`,
`controller.rs`.

## To regenerate after merging

- TypeScript bindings (`scripts/gen-bindings.sh`): new `VoiceFilter`,
  `VoiceFilterMode`, `TargetLfo`, `ChannelTargetLfos`, `LayerSettings`,
  `SamplerPreprocessRequest`; changed `ChannelEnvelopes`,
  `ChannelVoiceSettings`, `LfoTarget`.
- The document WASM (`scripts/build-sim.sh`): the project crate changed, and
  the mounted UI tests above need it.
- Binding fixtures, if any embed a channel's voice.
