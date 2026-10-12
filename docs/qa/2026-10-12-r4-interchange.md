# Round-four interchange hand-off — 2026-10-12

Worktree: `C:/Users/ewhee/.t3/projects/windfall/.claude/worktrees/r4-interchange`.
Branch: `gpt/r4-interchange`. Base: `cfd2d37ee5af6ce8dc3f55acd539ba4929b0945b`.

**Partial implementation delivery. All three rows remain in progress.** This
branch does not remove every software gap in the brief. In particular it does
not implement the full DrumSynth model, SYN/speech readers, all remaining FLP
processor/controller/timing translations, or audio-editor preservation of the
new slice attribute notes. Archival retention is not playback equivalence.

Nothing was built and no tests, applications, audio renders, generators,
installers, downloads or hardware workflows were run. Source inspection,
`git diff --check` and the contract's allowed single-file
`rustfmt --check --edition 2024 --config skip_children=true <changed-file>`
were performed. The Rust 2024 checks reported no syntax errors, but formatting
differences remain and several checks exited 1. An initial formatter check used
the implicit edition and rejected existing Rust 2024 let chains; those checks
were repeated with the correct edition. None of this is typecheck/test evidence.
All tests below are written, registered and nonignored, not executed.

The explicitly requested contract was read first. All subsequent source/docs
reads and edits stayed in this worktree. No subagents, pushes, PRs, tags,
dependency changes, vendor presets, third-party project fixtures, proprietary
binary inspection, tracker edits or generated artifact edits were made.
Commits used an empty/nonexistent hooks directory to prevent implicit builds.

Implementation commits:

- `a52ba275`: original FLP control/state event archive and single-pass recorded
  control conversion.
- `22ff02df`: WAV notes, two-source slice interchange, sound attributes, labels,
  fractional beats and navigation metadata.
- `755143d8`: original DrumSynth text ownership before import and portable saves.

## fmt-export-wav-markers

Changed files:

- `crates/windfall-codec/src/{wav_metadata,lib}.rs`: RIFF `adtl/note` read/write
  with cue linkage, UTF-8/NUL/text/count/aggregate bounds, preserving multiple
  notes per cue. Existing cue/label/region/smpl/ACID interfaces stay in place.
- `crates/windfall-dsp/src/granular/asset.rs`: additive default-empty, optional
  `SliceRegion.label`, validated at 256 UTF-8 bytes. Runtime voices still derive
  only numeric controls from prepared immutable assets; label strings are not
  cloned on note-on or parameter updates.
- Native `session/{sample_wav_metadata,sample_playback}.rs`: the existing native
  Import WAV slices / Export source WAV with slices UI actions now carry gain,
  pitch, attack/release, reverse, deck, authored labels and exact fractional
  source beat length. Both decks become one WAV, with B regions rebased after A.
  Different rates/channels use measured deterministic linear resampling/mono
  replication. Native import assigns A and B to that shared WAV. Shared-source
  re-export writes it once, rather than doubling the audio. Work is off-lock,
  with the existing source/document/preparation guards and checked history.
- `session/export_markers.rs`: emits loop/skip/pause descriptions as versioned
  notes on the actual finite linear render grid and writes fractional beat
  descriptions. Existing engine `play_passes` disables interactive navigation;
  this is preserved. Export no longer refuses those musical annotations merely
  because interactive playback could jump/pause. It does not execute navigation
  or claim physical jump/pause rendering. Standard `smpl` loops remain present.
- `docs/audio/wav-interchange.md`: exact custom-note grammar, standards versus
  extension distinction, frame semantics, resampling, limits and remaining work.

New/strengthened exact tests for the integrator:

- `crates/windfall-codec/tests/wav_markers.rs`:
  `riff_notes_have_independent_utf8_lengths_padding_ids_and_strict_reader_bounds`.
  Independent RIFF walker checks note text/IDs/lengths/padding; ordinary PCM
  decode and hostile/orphan/unterminated notes are asserted. Existing
  `cue_labels_slices_loops_and_tempo_have_independent_expected_bytes` stays intact
  with its exhaustive metadata initializer updated.
- `session/sample_wav_metadata.rs`, registered through sample_playback:
  `two_sources_labels_sound_attributes_and_fractional_beats_survive_cue_frame_roundtrip`;
  `different_source_rates_and_channels_have_measured_linear_resampling`.
  Existing `hostile_missing_overflow_and_subframe_regions_are_refused_before_export`
  retains all invalid-frame assertions and checks missing B through the new
  two-source helper.
- `session/tests/sample_playback.rs`:
  `native_two_deck_wav_interchange_restores_attributes_labels_beats_history_and_shared_owned_source`.
  Independent on-disk note/label/region inspection, measured PCM, two-source
  import, undo/redo, identical shared-source re-export and save/reopen.
- `crates/windfall-project/tests/sample_playback.rs`:
  `authored_slice_labels_are_checked_persisted_undoable_and_default_empty_in_old_projects`.
  Explicit old-project field removal, checked rejection and unchanged history.
- `session/export_markers.rs`:
  `navigation_actions_have_notes_on_the_finite_linear_render_grid`, replacing
  the obsolete navigation-refusal policy test with skip/pause frame assertions.
- `session/tests/export.rs`:
  `song_wav_navigation_notes_preserve_actions_and_linear_pcm_positions_in_full_and_region_exports`,
  replacing the obsolete refusal-policy test with successful output, audible PCM
  after the source skip, actual cue frames, independent note bytes, fractional
  region beats, omitted out-of-range actions and unchanged document assertions.

Complete in source, pending integration: native SliceDeck two-source region and
sound-attribute import/export; label persistence; fractional beats; bounded RIFF
notes and navigation annotation export. WavPack/M4A and codec registration were
not changed.

SOFTWARE: preserve these slice attributes/labels/beat/navigation descriptions
through the audio editor's source/view/edit/transfer workflows; those workflows
currently write WAV without this new annotation bridge. Provide action-aware
navigation import where the workflow requires native timeline restoration, and
the additional corresponding editor/slicer round-trip fixtures. Source A/B are
coalesced into one sample rather than restored as separate original-rate files;
different-rate export is explicitly linearly resampled. Confirm that this
documented interchange contract satisfies acceptance. UI label authoring is
still unavailable; imported labels are retained. No expanded generated-voice
note cue feature was added. Run integration validation before promotion.

EXTERNAL-ONLY: other applications' interpretation of standard WAV chunks and
Windfall extension notes, non-UTF8 label compatibility, resampling/listening
comparisons. Third-party applications are not claimed to understand our notes.

## fmt-import-legacy-presets

Changed files:

- New registered native `session/preset_assets.rs`: bounded original `.ds` text
  reads, deterministic PCM/source-version comparison, staged no-clobber copy to
  persistent app-owned storage before document installation, and owned-file
  cleanup on rejected imports. The original preset is preserved, not rewritten
  as a WAV or serialized from a subset of known fields.
- `session/library.rs`: common native browser/file import flow uses that owned
  source; repeats original source/token/byte/version checks after preparation;
  preserves existing document/generation/history/publication guards. Only
  successful installed files enter the existing derived-asset registry.
- `session/files.rs`: first save adopts owned preset text into relative
  `Presets/` project assets via the existing staged copier and history-wide
  source relink. Save As then uses ordinary project-source carrying. It does not
  collect arbitrary external audio or unrelated app files.
- `session/mod.rs`: registers the new source ownership module.

Exact tests, all in `session/tests/import_formats.rs`:

- New `owned_preset_survives_original_deletion_first_save_move_history_and_fresh_reopen`:
  exact original text, deletion before first save, project-relative ownership,
  undo/redo, move, removal of old copies, fresh reopen and closed-form measured PCM.
- Strengthened
  `drumsynth_native_sampler_import_reports_limits_and_reopens_original_preset_without_rewriting_it`:
  now additionally checks exact project-owned original text after save and the
  correctly relinked portable project on reopen.
- Existing
  `drumsynth_source_changes_replacement_and_unsupported_synthesis_never_install_stale_or_wrong_audio`
  remains registered and unweakened; its decoded/prepared source barriers also
  cover the new ownership path.

Complete in source, pending integration: supported DrumSynth source text is
owned before installation, carried into project-owned assets on save, and no
longer depends on the importer's original external file.

SOFTWARE: full public DrumSynth overtones/noise bands/slope/droop/filter/distortion
synthesis remains unimplemented; the codec's existing approximate tone/flat
noise subset and honest refusal of active unsupported sections are unchanged.
SYN and speech public layouts/readers/renderers remain absent. No layout or
synthesis formula was guessed from filenames or an unrelated native instrument.
Their portable ownership must reuse/extend the copy path once their actual
readers exist. Additional synthetic model/reference and hostile-input tests are
still needed, followed by integration validation.

EXTERNAL-ONLY: lawful independent preset corpus/reference sound comparisons and
listening after those implementations exist. Missing readers/model behavior
are software work, not an external-only or licensing deferral.

## fmt-flp-import

Changed files:

- `crates/windfall-flp/src/{model,parse}.rs`: bounded original event stream,
  excluding personal licensee event 200; retention stops at complete events with
  an overflow diagnostic, while ordinary conversion continues.
- `src/convert/{mod,patterns}.rs`; obsolete `pattern_events.rs` removed: saved
  valid FLhd/FLdt source archive includes owner-selection context, unsupported
  recorded controls, processor states, controller/formula/smoothing bytes,
  nonlinear timing and navigation. Existing bounded event-223 conversion runs
  once; the duplicate older converter no longer attempts rejected edits or
  double-reports supported controls. Understood effect parameter mappings stay.
- `crates/windfall-project/src/{model,check}.rs`: additive/default-empty/optional
  `retainedSourceData` with source/format/data, separate from retainedPlugins;
  count/name/aggregate 16 MiB plus container header bounds on reopen. No archive
  bytes are executed or passed to a plugin. Ordinary project replacement and
  subsequent document edits/history/save/reopen preserve this data.
- `docs/flp/coverage.md`: appended truthful unverified archival coverage and gaps.

Exact tests:

- `crates/windfall-flp/tests/conversion.rs`:
  `unsupported_control_controller_timing_and_navigation_bytes_are_durable_with_owner_context`;
  `source_event_archive_excludes_licensee_and_stops_at_complete_events_with_a_budget_warning`.
  Original synthetic event 223/227/89/148/250 streams, archival container equality,
  parsed values, owner context, warning visibility, JSON reopen, private metadata
  exclusion and complete-event overflow are asserted.
- Strengthened existing
  `recorded_pattern_controls_import_from_real_bytes_as_held_lanes` rejects the
  redundant-converter warning; all earlier recorded-control tests remain.
- `crates/windfall-project/tests/sample_playback.rs`:
  `opaque_import_archive_bounds_and_legacy_defaults_are_checked_on_reopen`.

Complete in source, pending integration: unsupported controls from actual parsed
files are durably retained with context and warnings, and understood recorded
controls have one authoritative converter. No synthetic fixture is presented as
new native plugin-state decoding or real-version corpus evidence. Direct
`convert()` on a manually constructed FlpProject without original event bytes
cannot archive bytes the caller did not provide; native/file `import()` does.

SOFTWARE: the remaining processor states, instrument parameter automation,
controller formulas/smoothing, nonlinear shift/gate/glide and arrangement
navigation translations remain incomplete. The checkout's cited layouts do not
provide their missing parameter encodings/formulas; none was invented. Archive
overflow retains a warned complete-event prefix rather than all source bytes.
Compile and run full reader/conversion/project/native import acceptance.

EXTERNAL-ONLY: compatibility checks against a lawful corpus of real projects
from additional versions and reference-product sound/timing comparisons. No
third-party project or reference-product binary was read or added in this round.

## Integrator validation

Run serially in the integration checkout, with the normal external
TS_RS_EXPORT_DIR and MSVC environment:

- `cargo test -p windfall-codec --test wav_markers`, then full codec tests.
- `cargo test -p windfall-dsp --test dsp sample_playback`, including allocation
  guards, and existing granular unit tests.
- `cargo test -p windfall-project --test sample_playback`, then full project tests.
- `cargo test -p windfall-flp`, including all existing conversion/model/property
  tests and the new archive tests.
- `cargo test -p windfall-desktop --lib session::tests::sample_playback`.
- `cargo test -p windfall-desktop --lib session::sample_playback::wav_metadata`.
- `cargo test -p windfall-desktop --lib session::export::markers`.
- `cargo test -p windfall-desktop --lib session::tests::export`.
- `cargo test -p windfall-desktop --lib session::tests::import_formats`.
- Existing native file carrying/history, sample cache, stale imports, FLP,
  sample-playback workflow and editor suites, followed by the merged full suites.

Apply the ordinary formatting/lint checks to the touched packages. Regenerate
Project/RetainedSourceData/SliceRegion bindings and simulator through the normal
integrator workflow before frontend typecheck and existing mock-backed sample
playback/FLP/inspector/document suites. No Command, registry, descriptor index,
choice order or codec extension list was changed. There are no new dependencies.

## Least certain spots

1. Native `preset_assets.rs`: tempfile persist/no-clobber ownership inference,
   exact cache provenance after copying, cleanup and original-file race tests
   have not been compiled. Verify the production dependency's exact API and
   decoded/prepared stale-import tests first.
2. First-save External-to-Project preset relinking through `files.rs`:
   history-only/undo/redo roots and concurrent saves need the full native file
   tests. New portable tests are unexecuted; do not weaken assertions if they fail.
3. WAV combined-source normalization/re-export precision, different source-rate
   linear resampling and preservation of MIDI vector order/overlaps need actual
   native playback and the new byte/PCM tests. This combines sources; it does
   not preserve separate original file formats/rates or original float bounds.
4. Finite song export really uses `play_passes`, disabling navigation in the
   inspected engine. The new native navigation PCM/frame assertions still need
   execution, including stems, tempo maps and selected regions. This is musical
   action annotation interchange, not action-performing offline rendering.
5. The optional ts-rs annotations on SliceRegion.label and retainedSourceData,
   legacy serde defaults and generated UI/mock types need fresh generation.
   No generated artifact or frontend implementation was changed by this worker.
6. Full source archival increases saved JSON and snapshot size (bounded to
   16 MiB raw events) and is retained only by file parsing. Verify report counts,
   native preview/open/reopen behavior and hostile container limits. Unknown
   controls remain playback omissions even when archived.
7. Single-file syntax/format inspection is not Rust type checking. Formatting
   differences remain. Nothing was built and no tests or application/audio
   workflows were run; compilation, lint and measured assertions may need repair.

HAND-OFF COMPLETE
