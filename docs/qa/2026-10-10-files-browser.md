# Project files, presets, templates and browser — 2026-10-10

Stream T5 hand-off for `fmt-project-zip`, `fmt-save-new-version`,
`fmt-project-templates`, `fmt-state-fst`, `fmt-score-fsc`,
`wf-project-settings`, `win-browser-current-project`,
`win-browser-project-picker`, `win-browser-plugin-database`,
`win-browser-plugin-picker` and `fmt-host-plugin-manager`.

Branch `worktree-agent-adbae3149b4aeee63`. This file is kept current while
the work goes on; the status line of each row says how far it is.

## How to read the evidence

Part of this work was done under a rule that forbids building and testing in
the worktree. Everything is therefore in one of two states, and each item
says which:

- **Executed**: the command ran here, on Windows, before the rule, and the
  counts are what it printed.
- **Written, not executed**: source and tests are in the tree and have never
  been compiled. The parent builds, runs and repairs them after merging. A
  row whose only new evidence is in this state is not claimed as complete.

Nothing was built, tested, linted or generated after the rule came in. No
bindings, simulator module or other generated artifact is in any commit.

### How the executed commands were run

The brief prescribes Git Bash with `source scripts/msvc-env.sh`. The harness
this agent ran in refused `source` in the Bash tool (it could not verify what
the sourced text runs), so cargo was run from the PowerShell tool through a
temporary helper outside the repository that loads the same Visual Studio
Build Tools environment (`vcvars64.bat` of the 2022 Build Tools) and then
calls cargo with `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`, debug info off
and `TS_RS_EXPORT_DIR` pointing at a temporary folder. The helper ran cargo
and nothing else. The parent should rerun the commands the prescribed way.

## Status by row

| Row | Claim | Evidence state |
| --- | --- | --- |
| `fmt-project-zip` | Candidate for full row once the two commands are rerun on the merged tree: the gates the design docs left open were executed before this stream refactored `session/archive.rs` | Executed, on an earlier tree |
| `fmt-save-new-version` | Candidate for full row on the same condition | Executed, on an earlier tree |
| `wf-project-settings` | Still partial: model, engine, MIDI and FL import are written; there is no control for the two settings in the UI | Written, not executed |
| `fmt-state-fst` | Still partial: format, commands, files and library are written; there is no UI and no TypeScript backend method | Written, not executed |
| `fmt-score-fsc` | Still partial: format, MIDI conversion and files are written; there is no UI and no TypeScript backend method | Written, not executed |
| `fmt-project-templates` | Still partial: factory and user templates are written natively; there is no UI and no TypeScript backend method | Written, not executed |
| `win-browser-plugin-database` | Still partial: the saved database, its view and its commands are written; there is no browser tab | Written, not executed |
| `fmt-host-plugin-manager` | Still partial: folder removal, rescans and a verification status per plugin are written; the manager dialog does not call them yet | Written, not executed |
| `win-browser-plugin-picker` | Not started; it would be a view over the plugin database above | Nothing new |
| `win-browser-current-project` | Not touched by this stream | Nothing new |
| `win-browser-project-picker` | Not touched by this stream | Nothing new |

No row of this stream is claimed complete on written-only evidence. The
frontend was stopped short on purpose when the no-build rule and the usage
limit came in: TypeScript against bindings that cannot be regenerated or
type-checked here would have been guesswork. Every native piece is reachable
through a registered IPC command, so the UI work that remains is wiring and
views, listed per row below.

## `fmt-project-zip` and `fmt-save-new-version`

No source was changed for these two rows. Their notes and
`docs/PORTABLE-REPAIRS.md` left two things open: the combined rerun of the
portable suites on the integrated tree, and the real CLAP archive fixture,
which round three excluded and left to the parent. Both ran here, on the
branch base `fd60cdde` plus one additive change to `windfall-core` (a new
`PanLaw` type that nothing used yet).

| Command | Result |
| --- | --- |
| `cargo test -p windfall-archive --lib --test archive` | exit 0; the `archive` integration target printed 15 passed, 0 failed. The count of the `--lib` parser target scrolled out of the captured output; `PORTABLE-REPAIRS.md` records 6 for it. |
| `cargo test -p windfall-desktop --lib -- session::tests::archive session::tests::versions session::tests::files session::files::tests` | exit 0; 53 passed, 0 failed, 0 ignored, 356 filtered out, 51.10 s |

The 53 include `selected_native_plugin_capture_archive_and_export_round_trip`,
which built the CLAP test plugin and the desktop scanner helper itself and
then saved a numbered version, exported an archive, reopened it and exported
audio with the native state intact. They also include the round-two and
round-three repair regressions by name:
`portable_concurrent_carries_never_replace_the_other_versions_audio`,
`portable_source_conflict_refuses_root_change_without_discarding_history`,
`portable_history_only_missing_source_stays_missing_after_move`,
`portable_move_keeps_a_sample_undone_before_capture`,
`portable_move_keeps_a_sample_undone_during_save`,
`portable_move_keeps_a_sample_added_and_undone_during_save`,
`backup_shaped_numbered_destination_carries_relinks_and_reopens_audio`,
`independent_saving_owners_race_atomic_publication_without_overwrite` and
`same_session_saves_are_serialized_and_process_competitor_is_not_overwritten`.

Not rerun here: the frontend suites for these rows
(`src/lib/flows/portable.test.ts`, `src/features/layout/archive-report.test.tsx`),
because the simulator module is not in the tree and the rule came in before
it could be built. Their sources are unchanged by this stream.

Remaining gates, all external: cross-machine throughput on large projects,
power-loss behaviour on specific filesystems, macOS and Linux dialogs, and a
third-party plugin corpus.

The parent must rerun the two commands on the merged tree before promoting
either row: a later commit of this stream splits two functions of
`session/archive.rs` so that templates can reuse them (see
`fmt-project-templates` below). That split is written, not executed.

## `wf-project-settings`

Written, not executed.

**What it is.** Two project settings beside tempo, meter and swing, both
stored in the project file with defaults that leave older files unchanged,
both changed with `UpdateSettings` as one named undo step.

- **Panning law** (`settings.panLaw`: `balance`, `constantPower`,
  `compromise`, `linear`). Every law is unity at the center, so a project in
  which nothing is panned sounds the same under all four and pan stages in
  series do not add up to a level change. `balance` is what the engine did
  before and is the default. The engine applies the law to a channel's pan, a
  sampler note's pan, an audio clip's pan and a mixer track's pan, live and
  in an export, through the plan (`Plan.pan_law`), with no allocation, lock
  or IO added to the audio path.
- **Timebase** (`settings.timebasePpq`: 24, 48, 96, 120, 192, 240, 480,
  960). Stored positions stay at 960 ticks to the quarter note; the setting
  is the grid that playback puts notes and playlist clips on. The engine
  compiles its plan from `timebase::playback_project`, an export compiles
  the same plan, and MIDI export writes the same project at the timebase's
  own resolution (`ppq: 0` in `MidiExportOptions` means "the project's").
  Lowering the timebase loses nothing: raising it again plays the stored
  timing. `crates/windfall-project/src/timebase.rs` documents the rules.
- **Import.** An FL Studio project that names its panning law gets it
  (circular becomes `constantPower`, triangular `balance`). A file's own
  time resolution is converted to 960 as before and is not adopted as the
  project's timebase.

**Tests written.**

- `crates/windfall-core/src/lib.rs`:
  `every_pan_law_is_unity_at_the_center_and_silences_the_far_side_at_the_end`,
  `pan_laws_keep_what_their_names_promise`.
- `crates/windfall-project/src/project_settings.rs`, `src/timebase.rs`: unit
  tests.
- `crates/windfall-project/tests/project_settings.rs`:
  `a_file_that_names_neither_setting_has_the_balance_law_and_960_ticks`,
  `each_setting_is_one_named_undo_step_and_survives_a_file`,
  `a_timebase_windfall_does_not_have_is_refused_by_the_command_and_by_the_file`,
  `playback_puts_notes_and_clips_on_the_grid_and_leaves_the_document_as_drawn`,
  `the_grid_positions_are_the_exact_ones_the_rounding_rule_gives`.
- `crates/windfall-engine/tests/engine/project_settings.rs` (registered in
  `tests/engine/main.rs`):
  `every_pan_the_engine_applies_follows_the_projects_law`,
  `pan_stages_in_series_multiply_and_a_centered_project_sounds_the_same_under_every_law`,
  `the_balance_law_is_what_projects_played_with_before_the_setting`,
  `changing_the_law_while_playing_takes_effect_and_never_touches_the_allocator`,
  `an_export_renders_exactly_what_plays_under_every_law_and_timebase`,
  `notes_and_clips_sound_on_the_grid_of_the_timebase`.
- `crates/windfall-ipc/src/midi.rs`:
  `midi_export_writes_what_plays_at_the_project_timebase`.
- `crates/windfall-midi/tests/score.rs`:
  `a_midi_file_at_the_project_timebase_holds_what_plays_without_rounding`.
- `crates/windfall-flp/tests/conversion.rs`:
  `the_panning_law_a_file_names_becomes_the_projects_law`.

**Commands for the parent.**
`cargo test -p windfall-core --lib pan`,
`cargo test -p windfall-project --lib --test project_settings`,
`cargo test -p windfall-engine --test engine project_settings`,
`cargo test -p windfall-ipc --lib timebase`,
`cargo test -p windfall-midi --test score`,
`cargo test -p windfall-flp --test conversion panning`.
The whole engine, MIDI and FLP suites must be rerun too: the engine's pan
path and plan compile changed for every project.

**Remaining gates.**

- No UI: the project settings surface has no control for either setting.
  Both are plain `updateSettings` fields, so the control is a pair of
  selects. The MIDI export dialog still offers 480 and 960 only.
- A note's pan inside a built-in synth's own voices, and inside a hosted
  plugin, is that instrument's panning and does not follow the law. The
  audio editor's rendered view (`engine/src/audio_edit.rs`) still applies a
  clip's pan with the balance law. Closing either means passing the law
  into the instrument trait or into `render_view`, which other streams own.
- Changing the law while audio plays switches the gains of panned material
  at once, without a ramp.
- FL Studio's circular law is mapped to the nearest law here. No claim of
  identical levels is made.

## `fmt-state-fst`

Written, not executed.

**What it is.** One preset file type, `.wfpreset`, with a common envelope:
`schema` (`windfall.preset`), `version` (1), `kind` (`channel`, `plugin`,
`mixerTrack`), `name`, an `assets` manifest and the one body the kind
names. `crates/windfall-project/src/preset.rs` documents it.

- **Channel preset**: level, pan, voice and timing settings, and the sound:
  sampler settings with their audio, a built-in instrument, or a hosted
  instrument with its opaque native state.
- **Plugin preset**: the state of one built-in or hosted instrument or
  effect.
- **Mixer track preset**: the mixer stream's `MixerTrackPreset`, carried
  unchanged as a body and loaded with the mixer's own
  `ApplyMixerTrackPreset`. Nothing of the mixer stream's code was changed.
- **No project ids in a file.** A hosted binding is stored against a
  placeholder target and pointed at the destination on load. A sampler's
  audio is named by its manifest entry, with a path relative to the preset
  file; the shell copies it into `<preset name> audio/` beside the preset.
- **Loading is one command and one undo step**: `ApplyChannelPreset` and
  `ApplyPluginPreset`. Both take the destination as it was when the preset
  was chosen and refuse one that changed. The channel keeps its id, group,
  mute, solo and mixer track; an effect keeps its slot, mix and bypass. A
  sampler preset's audio file joins the sample pool inside the same
  command. Both commands make their edits and then run the project check,
  so a preset is held to exactly what a project file is, and a refused
  preset changes nothing.
- **Automation**: automation of a setting addressed by index survives only
  when the processor stays the same (same built-in kind, or same hosted
  plugin with the same parameter id at that index). The rest goes, with its
  clips, in the same undo step. Automation of what the destination keeps
  (channel volume and pan, effect mix) is not touched.
- **Older and newer files**: a newer preset version is refused by name. For
  built-in processors a renamed setting is carried over (`PARAM_RENAMES`,
  empty today because nothing has been renamed yet), an unknown setting is
  dropped, a missing one takes its default and an out-of-range value is
  clamped, each with a line in the review.
- **Missing things**: missing audio is listed and refuses the load unless
  the user asks to load the settings without it. A hosted plugin that is not
  installed is reported; its state is kept whole and it stays silent or
  bypassed, as in a project.
- **Capture** checks the generation and revision the caller saw and captures
  live native state with the document lock released, like the mixer
  stream's capture.
- **Preset library**: `<config>/presets/*.wfpreset`, with save, list and
  delete. Each entry says which plugin it belongs to, which is what the
  plugin database lists presets by.

**Tests written.**

- `crates/windfall-project/tests/presets.rs`:
  `a_sampler_channel_preset_carries_its_audio_and_loads_as_one_undo_step`,
  `a_presets_audio_file_joins_the_pool_in_the_same_step_and_is_not_added_twice`,
  `name_and_color_are_restored_only_when_asked`,
  `automation_survives_the_same_instrument_and_goes_with_another`,
  `a_hosted_instrument_keeps_its_opaque_state_and_follows_the_destination`,
  `a_plugin_preset_replaces_one_effect_and_leaves_its_slot_alone`,
  `a_stale_destination_a_wrong_role_and_invalid_bodies_are_refused_whole`,
  `a_hosted_destination_is_recognised_by_its_plugin_not_by_its_changing_state`,
  `the_envelope_refuses_other_schemas_newer_versions_and_unsafe_or_repeated_assets`,
  `settings_from_another_build_are_renamed_dropped_defaulted_and_clamped_with_a_report`,
  `a_mixer_track_preset_travels_in_the_same_envelope_unchanged`.
- `apps/desktop/src-tauri/src/session/presets.rs`: unit tests for file
  names, the extension and asset paths.
- `apps/desktop/src-tauri/src/session/tests/files_qa.rs` (registered in
  `session/tests/mod.rs`):
  `a_sampler_preset_takes_its_audio_along_and_loads_as_one_undo_step`,
  `missing_audio_is_reported_and_loads_only_when_asked_to`,
  `a_capture_for_a_project_that_moved_on_is_refused`,
  `plugin_presets_keep_a_missing_plugins_state_and_the_library_lists_them`.
- `apps/desktop/src/lib/ipc/sim/document.test.ts`: entries for
  `applyChannelPreset` and `applyPluginPreset` in `EVERY_COMMAND`.

**Commands for the parent.**
`cargo test -p windfall-project --test presets`,
`cargo test -p windfall-desktop --lib -- session::presets::tests session::tests::files_qa`,
then with fresh bindings and simulator
`pnpm --dir apps/desktop test src/lib/ipc/sim/document.test.ts --maxWorkers=4`.

**Remaining gates.**

- No UI and no `Backend` methods. The commands are `preset_capture_channel`,
  `preset_capture_plugin`, `preset_write`, `preset_read`,
  `preset_apply_channel`, `preset_library`, `preset_library_save` and
  `preset_library_delete`. A plugin or mixer preset is applied by
  dispatching its command with the body `preset_read` returned.
- The browser preview has no disk, so presets with audio need the desktop
  app; the mock side is not written.
- A live native-state capture into a preset is covered only by the capture
  code it shares with saving. A test with the real CLAP fixture would call
  `preset_capture_plugin` after `super::plugins::manager(&rig)`.
- Presets with a real third-party plugin, and other platforms, are external.

## `fmt-score-fsc`

Written, not executed.

**What it is.** A score file, `.wfscore`: `schema` (`windfall.score`),
`version` (1), a name, the length, an optional meter, the notes of one lane
with velocity, pan and expression, and the expression curves drawn on them,
all at 960 ticks to the quarter note and with no project ids.
`Score::capture` takes a lane or a selection of its notes;
`Score::load_command` builds one undo step from ordinary commands that
replaces or merges, gives every note a new id, lets the curves follow their
notes and lengthens a pattern that is too short. A score also travels as a
Standard MIDI File: `windfall_midi::score_to_song` and `score_from_song`,
each returning a line for everything the other form cannot hold. The shell
writes either form by extension and reads either by content.

**Tests written.**

- `crates/windfall-project/tests/scores.rs`:
  `a_score_holds_a_lanes_notes_and_curves_and_nothing_of_the_project`,
  `loading_replaces_or_merges_in_one_undo_step_with_new_note_ids`,
  `files_that_are_not_readable_scores_are_refused_with_the_reason`.
- `crates/windfall-midi/tests/score.rs`:
  `a_plain_score_survives_a_midi_file_note_for_note`,
  `what_a_midi_file_cannot_hold_is_reported_and_the_notes_still_travel`,
  `any_midi_file_loads_as_one_lane_with_a_report_of_the_rest`,
  `files_a_score_cannot_hold_are_refused_or_cut_with_a_reason`.
- `session/tests/files_qa.rs`:
  `a_score_is_written_as_a_score_or_a_midi_file_and_read_back_from_either`.

**Commands for the parent.**
`cargo test -p windfall-project --test scores`,
`cargo test -p windfall-midi --test score`,
`cargo test -p windfall-desktop --lib session::tests::files_qa`.

**Remaining gates.**

- No UI and no `Backend` methods. The commands are `score_write` and
  `score_read`. The piano roll needs Save score and Load score actions that
  build the score from the lane or selection it shows and dispatch the load
  command. In the browser preview that needs either a simulator export for
  `Score::capture` and `Score::load_command` or the same few lines in
  TypeScript.
- A score written as MIDI leaves out a note's color group and articulation;
  the MIDI stream's expression export could carry them later.

## `fmt-project-templates`

Written, not executed.

**What it is.** `apps/desktop/src-tauri/src/session/templates.rs`.

- **Factory templates** (`factory:empty`, `factory:basic-kit`,
  `factory:four-on-the-floor`, `factory:boom-bap`) are built by code from
  Windfall's own factory sounds each time. There is no file to edit,
  replace or delete, and saving a user template under one of their names is
  refused.
- **User templates** are the open project as a portable archive in
  `<config>/templates/<name>.wftemplate`, so they carry their audio and
  plugin state. Saving one leaves the open project exactly as it is and
  never overwrites an existing template.
- **A project started from either** is called "Untitled", has no file, has
  an empty undo history and is not marked changed. It goes through the same
  guarded replacement as New and Open.

To reuse the tested archive code, `session/archive.rs` was split without
changing what it does: `project_archive_save_with` now ends in
`archive_export_to(target, job)`, and `project_archive_open_with` calls
`archive_open_adjusted(path, job, what, adjust)`, which checks the adjusted
project before the existing guarded install. The archive path's messages
are the same text as before.

**Tests written.**

- `session/templates.rs`:
  `every_factory_template_is_a_valid_project_with_no_history`,
  `the_beats_light_the_steps_they_describe`.
- `session/tests/files_qa.rs`:
  `factory_templates_start_fresh_projects_and_cannot_be_deleted_or_shadowed`,
  `a_user_template_carries_its_audio_and_opens_as_a_new_project`.

**Commands for the parent.**
`cargo test -p windfall-desktop --lib -- session::templates::tests session::tests::files_qa session::tests::archive`.

**Remaining gates.**

- No UI and no `Backend` methods. The commands are `templates`,
  `template_save`, `template_new` and `template_delete`. The File menu
  needs New from template and Save as template; New from template must ask
  about unsaved changes first, as New does.
- Each project started from a user template leaves its extraction under
  `<config>/projects/`, as an opened archive does. Pruning those is the
  lifecycle work `PORTABLE-PROJECTS.md` already lists.
- No test races an edit against a template being opened; the install guard
  it relies on is covered by the archive tests.

## `win-browser-plugin-database`, `win-browser-plugin-picker`, `fmt-host-plugin-manager`

Written, not executed. Native and IPC only.

**What it is.**

- `crates/windfall-ipc/src/files.rs`: `PluginDatabase`, the saved file, and
  `PluginDatabaseView`, what a browser or picker shows. The view lists
  Windfall's own instruments and effects, every plugin the last scan found,
  and the plugins of earlier scans that are missing now. Each record has a
  verification status (`builtin`, `verified`, `failed`, `blocked`,
  `missing`) with the scanner's message, whether it is an instrument, a
  category, and a favorite mark. The category is the user's choice or one
  worked out from the plugin's name; the favorite and the category survive
  a plugin going missing. A built-in's id is the `type` its settings have
  in files, which is also how a preset names its plugin, so presets can be
  listed under plugins by matching `PresetEntry.plugin`.
- `apps/desktop/src-tauri/src/plugin_database.rs`: the file
  `<config>/plugin-database.json`, written atomically. A damaged or newer
  file is left alone, reported, and never written over; a change that
  cannot be saved is taken back.
- `apps/desktop/src-tauri/src/plugins/manager_workflows.rs`: removing a
  folder the user added (the standard folders cannot be removed), retrying
  everything that is blocked, and verifying everything again from an empty
  catalog. Each ends in the ordinary isolated scan. No hosting or bridge
  code was changed; `plugins/mod.rs` gained the module declaration and one
  re-export.
- Commands: `plugin_database`, `plugin_database_annotate`,
  `plugin_database_forget_missing`, `plugins_remove_folder`,
  `plugins_rescan`. The database commands work without native hosting, with
  the built-ins only.

**Tests written.**

- `crates/windfall-ipc/src/files.rs`:
  `the_view_lists_builtins_and_every_scanned_plugin_with_its_status`,
  `favorites_and_categories_survive_the_file_and_a_plugin_going_missing`,
  `damaged_newer_and_invalid_input_is_refused`,
  `names_that_say_what_a_plugin_is_file_it_and_others_fall_back`.
- `apps/desktop/src-tauri/src/plugin_database.rs`:
  `scans_favorites_and_categories_are_saved_and_survive_a_restart`,
  `a_file_that_cannot_be_read_is_left_alone_and_reported`,
  `a_change_that_cannot_be_saved_is_taken_back`.
- `apps/desktop/src-tauri/src/plugins/manager_workflows.rs` (Windows):
  `a_folder_the_user_added_can_be_removed_and_a_standard_one_cannot`.

**Commands for the parent.**
`cargo test -p windfall-ipc --lib files`,
`cargo test -p windfall-desktop --lib -- plugin_database plugins::manager_workflows`.

**Remaining gates.**

- `win-browser-plugin-database`: the browser tab over `plugin_database`,
  with category and favorite editing and presets under each plugin.
- `win-browser-plugin-picker`: not started. A full-screen grid over the
  same view that adds the chosen plugin by double-click or drag:
  `pluginsAdd` for a hosted one, `addChannel` with an instrument or
  `addEffect` for a built-in.
- `fmt-host-plugin-manager`: the manager dialog does not call the new
  commands or show the status yet. No test runs `plugins_rescan` against
  the real scanner helper; with the CLAP fixture that is a rescan after
  `super::plugins::manager(&rig)`. macOS and Linux hosting stay external.
- The worked-out category is a guess from words in the name.

## `win-browser-current-project` and `win-browser-project-picker`

Not touched. The gap the inventory names stands: the Project tab is a
read-only list without control links or a way to act on a row, and the
overview is a dialog, not a full-screen picker. One thing found on the way:
the project model has no controller links (`wf-controller-linking` is
`todo`), so until that row lands the only links a Project tab can list are
automation targets.

## Where the new folders are

All beside the settings file, in the application's config folder:

| Path | What | Written by |
| --- | --- | --- |
| `presets/*.wfpreset`, `presets/<name> audio/` | This stream's preset library: channel and plugin presets, and mixer presets in the common envelope | `session/presets.rs` |
| `templates/*.wftemplate` | User templates | `session/templates.rs` |
| `plugin-database.json` | Favorites, categories and remembered plugins | `plugin_database.rs` |
| `projects/project-*` | Extractions of opened archives and templates | `session/archive.rs`, as before |

The mixer stream's per-user mixer preset library (`session/mixer_presets.rs`,
`mixer_preset_library*`, `lib/ipc/mixer-presets.ts`) landed on the
integration branch after this branch was cut and is not in this worktree.
This stream does not duplicate or call it. Two things for the merge:

- If the mixer library also keeps its files in a folder called `presets`,
  the two share it harmlessly as long as the extensions differ: this
  stream's listing reads only `*.wfpreset` and its delete refuses anything
  else. Check the folder name once, and rename `LIBRARY_FOLDER` in
  `session/presets.rs` if a clear separation is wanted.
- `preset_read` also reads a bare mixer preset file, so one preset browser
  can show both kinds later without converting any file.

## Shared seam files touched

All appended to, none reordered or reformatted.

- `crates/windfall-project/src/model.rs`: two fields at the end of
  `ProjectSettings` and their defaults in `Project::new`.
- `crates/windfall-project/src/command.rs`: `ApplyChannelPreset` and
  `ApplyPluginPreset` after `ReplaceNotebook`; two fields at the end of
  `SettingsPatch`.
- `crates/windfall-project/src/lower.rs`: `mod preset_apply;`, two match
  arms after `ReplaceNotebook`, two blocks in `update_settings`.
- `crates/windfall-project/src/check.rs`: one check in `check_settings`.
- `crates/windfall-project/src/lib.rs`: module declarations and re-exports
  at the end.
- `crates/windfall-ipc/src/lib.rs`: `mod files; pub use files::*;`.
- `apps/desktop/src-tauri/src/commands.rs`: nineteen paths at the end of
  the handler list. The commands themselves are in the new
  `files_commands.rs`.
- `apps/desktop/src-tauri/src/lib.rs`: two module declarations.
- `apps/desktop/src-tauri/src/session/mod.rs`: three module declarations.

Not on the seam list but shared with other streams:
`crates/windfall-engine/src/{plan,state,mixer,voice,clips}.rs` (the panning
law: one field or one argument at each pan call site),
`crates/windfall-midi/src/{write,lib}.rs`, `crates/windfall-ipc/src/midi.rs`,
`crates/windfall-ipc/Cargo.toml` (`serde_json` as a normal dependency; it is
a workspace dependency already in the tree, so no new licence),
`crates/windfall-flp/src/convert/mod.rs`,
`apps/desktop/src-tauri/src/shell.rs` (the database is managed at startup),
`apps/desktop/src-tauri/src/plugins/mod.rs` (two lines),
`apps/desktop/src-tauri/src/session/archive.rs`,
`apps/desktop/src-tauri/src/session/tests/mod.rs`,
`crates/windfall-engine/tests/engine/main.rs`,
`apps/desktop/src/lib/ipc/sim/document.test.ts`.

## What the parent must regenerate

- TypeScript bindings. New exported types: `PanLaw`, `Preset`, `PresetKind`,
  `PresetAsset`, `PresetAudio`, `PresetPluginIdentity`, `ChannelPreset`,
  `PluginPreset`, `PluginPresetState`, `Score`, `ScoreNote`,
  `ScoreLoadMode`, `PresetCapture`, `PresetReview`, `PresetEntry`,
  `ScoreReview`, `ScoreWritten`, `TemplateInfo`, `PluginStatus`,
  `PluginRecord`, `PluginAnnotation`, `PluginDatabaseView`. Changed:
  `Command`, `ProjectSettings`, `SettingsPatch`, `MidiExportOptions` (doc
  comment only).
- The simulator module: `windfall-core`, `windfall-project`, `windfall-midi`
  and `windfall-ipc` changed.
- `descriptors.json` is not affected: no effect or instrument changed.

## Least sure to compile or pass

Nothing was compiled after the first project-crate checkpoint, which
type-checked `windfall-core` and `windfall-project` before the tests, the
preset audio change and everything outside those two crates were written.
`rustfmt` parsed every new Rust file, so the risks are types and behaviour,
not syntax. In the order I would look:

1. `apps/desktop/src-tauri/src/commands.rs`: the handler list names the new
   commands by path (`crate::files_commands::name`). If `generate_handler!`
   does not take that form here, move the nineteen functions or import
   them.
2. `apps/desktop/src-tauri/src/files_commands.rs`: `AppHandle::try_state`
   for the optional plugin manager, and `Channel`, `Score` and
   `PresetCapture` as deserialized command arguments.
3. `apps/desktop/src-tauri/src/session/templates.rs`: the `const FACTORY`
   array of function pointers and the `[&[u32]; 4]` step literals.
4. `apps/desktop/src-tauri/src/session/tests/files_qa.rs`: it assumes that
   an ordinary dispatch of `ApplyChannelPreset` with a new audio file
   starts the background sample load (`wait_until_loaded_or_failed`, then
   `has_audio`), that a hosted binding dispatches with no plugin manager
   installed, and that a template's samples all come back as project paths.
5. `crates/windfall-engine/tests/engine/project_settings.rs`: exact levels
   to 1e-6 after the sampler, strip and master stages, exact impulse frames
   in song mode for a clip that starts off the grid, and zero allocator
   calls while a plan with another law is taken over.
6. `crates/windfall-midi/tests/score.rs`: equality of a song with itself
   after a write and read at 96 ticks to the quarter note, and whether the
   reader keeps a track name in a format 0 file.
7. `crates/windfall-project/tests/presets.rs`: the migration test picks the
   limiter's first two numeric settings by name order and uses -1.5 as a
   value inside both ranges; the effect test assumes setting 0 of the
   limiter takes -3.
8. `crates/windfall-project/src/preset.rs`: `ChannelPreset` uses
   `crate::channel_voice::deserialize_settings` and
   `#[ts(as = ..., optional)]` on two fields; the probe builds a `Channel`
   literal field by field.
9. `crates/windfall-flp`: an existing conversion test that pins a whole
   project or the settings of a file whose panning law is circular now
   sees `constantPower`.
10. `apps/desktop/src/lib/ipc/sim/document.test.ts`: the two new entries
    are typed against bindings that do not exist yet.

A field another stream adds to `Channel`, `MixerTrack`, `PluginBinding` or
`ProjectSettings` also has to go into the literals in
`crates/windfall-project/src/preset.rs` (the probe),
`crates/windfall-project/tests/presets.rs` and
`session/tests/files_qa.rs`.
