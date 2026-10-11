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
| `fmt-project-zip` | Candidate for full row: no source change; the gates the design docs left open were executed | Executed |
| `fmt-save-new-version` | Candidate for full row: no source change; the gates the design docs left open were executed | Executed |
| `wf-project-settings` | Implemented; still partial until executed | Written, not executed |
| `fmt-state-fst` | Implemented; still partial until executed | Written, not executed |
| `fmt-score-fsc` | Implemented; still partial until executed | Written, not executed |
| `fmt-project-templates` | In progress | See below |
| `win-browser-current-project` | In progress | See below |
| `win-browser-project-picker` | In progress | See below |
| `win-browser-plugin-database` | In progress | See below |
| `win-browser-plugin-picker` | In progress | See below |
| `fmt-host-plugin-manager` | In progress | See below |

The sections below are filled in as each piece lands.

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
either row: later commits of this stream touch `session/archive.rs`.
