# Round-five interchange hand-off — 2026-10-13

Worktree: `C:/Users/ewhee/.t3/projects/windfall/.claude/worktrees/r5-interchange`.
Branch: `gpt/r5-interchange`. Base: `469be35c`.

**Partial delivery across the assignment. Only the WAV editor/timeline row has
new implementation. FLP translation and complete legacy preset synthesis/readers
remain software work. No row is promoted or claimed built/passing.**

Nothing was built. No tests, applications, audio renders, generators, installers,
downloads, subagents, pushes, PRs or tags were run. No new dependencies, tracker
edits, generated bindings/simulator artifacts, reference binaries, real projects
or third-party presets are included. The explicitly requested contract was read
at its supplied parent-checkout path; all subsequent file reads/edits stayed in
the assigned worktree. Git hooks were disabled on each commit to prevent implicit
build/test/generation.

Checks performed: source/diff inspection and `git diff --check` (clean).
The allowed single-file `rustfmt --check --edition 2024 --config skip_children=true`
checks on the seven changed Rust files reported formatting differences (exit 1),
with no reported syntax errors. This is not typecheck/test evidence. Integration
formatting is still required. All tests listed below are written, registered,
nonignored and **not executed**.

Implementation commits:

- `8d49b98d`: document-owned annotations, native import/edit/transfer/export and
  timeline import, Rust fixtures.
- `62b2acad`: editor annotation authoring and export/timeline controls, backend
  adapters, mounted fixtures and interchange documentation.
- `ff509cd8`: exact beat notes govern the integer ACID beat display/control.

## fmt-export-wav-markers

Changed files:

- `crates/windfall-project/src/model.rs`: additive/default-empty
  `AudioRegion.notes`, bounded to 16 notes per annotation and 1024 UTF-8 bytes
  per note; rejects NUL in labels/notes. Existing `SetSampleRegions` checked
  edits retain the complete metadata through history and project serialization.
  No persisted enum/descriptor/choice order changed.
- `apps/desktop/src-tauri/src/session/audio_editor/wav_regions.rs`: new registered
  bridge for standard cue labels, ltxt regions, smpl loop kind/count/root, ACID
  tempo/meter/root/one-shot and arbitrary note text. Understood slice/beat/action
  extensions are checked; frame-bearing actions follow region coordinates.
  Original unknown notes are retained. Standalone smpl loops without cues acquire
  a labelled region. Oversized or malformed metadata is refused visibly.
- `session/library.rs`: ordinary native WAV file/browser imports read metadata
  off-lock and install annotations in the same prepared/checked import undo step.
  Existing source-version, generation, directory, loaded-source and publication
  guards still apply. Reusing an existing asset preserves its authored document
  annotations rather than overwriting them from disk.
- `session/audio_editor.rs`: coordinate transforms carry annotation notes;
  crop/extract scale exact beat counts by retained extent. Length-preserving and
  stretch maps preserve musical beat counts. Collapsed loop/slice/action regions
  become markers with a specific `Collapsed annotation:` note retaining the
  former attribute. Derived processing/transfer WAVs now contain the same
  annotations as their new document assets.
- New native `audio_editor_save_wav`: writes the actual rendered Float32 editor
  selection and its rebased annotations, without a document/history edit. Uses
  a successfully reserved staging file, cleanup ownership and final token/source/
  generation/edit/recording guards before destination publication.
- New native `audio_editor_import_timeline`: places markers using the current
  song clock (including tempo automation) and the clip start. Named regions
  produce start/end markers; skip/pause and forward infinite loops become native
  actions. One checked batch gives atomic refusal, undo/redo and save/reopen.
  Existing native navigation overlap/range checks remain intact.
- `src-tauri/src/commands.rs`, `src/lib/ipc/{backend,tauri,mock}.ts`: register both
  operations and preserve authenticated window calls. Browser DSP/file capability
  refusals are displayed; the mock does not claim to write WAV audio.
- `src/features/audio-editor/{regions,index,types}.tsx/ts`: visible label and
  endpoint editing; cue-note editing; slice gain/pitch/envelope/reverse/deck,
  exact beats/BPM, tempo/meter/root/one-shot and loop direction/count controls;
  labelled marker/region add, rename/save and delete; annotated selection WAV
  export and timeline import actions. Imported multiline notes survive label and
  typed attribute edits. Raw note-list editing explicitly treats lines as notes.
  Finite/directional sample loops show the specific named-boundary import limit.
- `docs/audio/wav-interchange.md`: coordinate/budget/standard-versus-extension
  contract and editor/native timeline behavior.

Exact Rust tests for the integrator:

- `crates/windfall-project/tests/audio_regions.rs`:
  `audio_region_notes_have_legacy_defaults_and_bounded_checked_history`;
  strengthened `audio_regions_have_guarded_atomic_history_and_default_compatible_persistence`.
  Checks old JSON without notes, UTF-8/NUL/count budgets, checked stale refusal,
  exact metadata history and JSON persistence.
- `apps/desktop/src-tauri/src/session/audio_editor/wav_regions.rs` (registered by
  `audio_editor.rs`):
  `note_frame_maps_preserve_unknown_notes_loop_types_counts_and_fractional_beats`;
  `malformed_annotations_and_overflow_are_explicitly_refused`.
  Measures reversed frame endpoints, multiline notes, alternating finite loops,
  exact fractional beats, malformed numbers/flags and annotation overflow.
- `apps/desktop/src-tauri/src/session/tests/audio_editor.rs` (existing registered
  session module):
  `wav_editor_annotations_import_rename_save_transfer_timeline_history_and_reopen`;
  `wav_annotation_invalid_or_stale_edits_never_publish_files_or_timeline_history`;
  `wav_annotations_follow_reversed_pitched_offset_view_and_baked_edit_frames`.
  Original synthetic WAV has known cue IDs/frames, notes, slice settings, beats,
  root/meter and loop/skip/pause. An independent RIFF walker checks cue frames,
  labels, note linkage/UTF-8, ltxt lengths, inclusive smpl bytes and ACID fields;
  measured Float32 PCM checks the exact selected frames. Native fixtures cover
  document/source distinction, label rename, owned sampler transfer, actual
  timeline action ticks, allocator-aware undo/redo, save/fresh reopen, hostile
  notes, stale publication and transformed/baked coordinates. Old test assertions
  remain; AudioRegion literals now include default-empty notes.

Exact frontend tests:

- `apps/desktop/src/features/audio-editor/second-pass.qa.test.tsx`:
  `authors cue labels notes and slice attributes through checked history and portable reopen`.
  Mounts real editor controls and dispatches metadata through WASM document
  commands; asserts authored labels/notes/attributes, single-step history,
  portable project reopen, deletion and undo. Requires fresh simulator/bindings.
- `apps/desktop/src/features/audio-editor/editor.test.tsx`:
  `exports the annotated WAV selection and shows native timeline capability errors`.
  Checks exact selection/path/token, displayed notes and truthful capability
  errors with unchanged project.
- `apps/desktop/src/features/audio-editor/tauri.test.ts`:
  strengthened `passes clip, token and exact selection; exposes native refusal`
  also checks both new authenticated native wire calls.

Implementation now covers the requested editor source/view/edit/transfer,
label authoring, annotated WAV saving and native timeline marker import paths.
Acceptance remains pending integration builds and all fixture/full-suite runs.
No known missing behavior from this row's round-four cell is intentionally left
as archival-only implementation.

Finite, alternating and backward **sample** loops have no corresponding native
song-direction/finite-count action: they retain their exact smpl settings in WAV
and editor, show a specific UI explanation, and import named timeline boundaries.
ACID source metadata does not replace the song clock. These distinctions are
documented rather than hidden behind a success claim.

EXTERNAL-ONLY: independent applications' interpretation of standard chunks and
Windfall note extensions; listening and independent Unicode/loop compatibility.
Unexecuted integration validation is not external-only evidence.

## fmt-flp-import

Changed files: none. New tests: none. The existing reader, archive, translated
effect tables and 3x Osc decoder are unchanged; no new playback equivalence is
claimed. Coverage and earlier hand-offs were read.

SOFTWARE: every round-four remaining item is still open in this delivery:
processor/instrument state beyond the already decoded tables; instrument
parameter automation for clips and recorded events; controller formula/smoothing
translation; nonlinear shift/gate/glide/portamento; region/loop/navigation marker
semantics and corresponding per-item reports plus synthetic event-stream tests.

The actual inspected evidence explicitly labels the default stand-ins' patch
layouts as undecoded (`convert/synth.rs`, `convert/effects.rs`, and the cited
instrument/effect seams). `model.rs`/`convert/voice.rs` explicitly lack the
nonlinear shift and Polyphony.slide equations; TimeMarker identifies plain/meter
codes but does not identify the other codes' semantics. This delivery has not
established the missing public layouts. It does not assert that lawful
implementation is impossible, that an archive satisfies the task, or that these
are merely external checks. No offset/parameter enumeration/formula was invented
from an opaque source blob or an unrelated Windfall descriptor.

EXTERNAL-ONLY: lawful real-project corpus from additional versions and independent
reference sound/timing comparison after the software implementation exists.

## fmt-import-legacy-presets

Changed files: none. New tests: none. Existing approximate tone/flat-noise render
and portable original `.ds` ownership remain unchanged and partial.

SOFTWARE: complete DrumSynth pitch droop/sweep, second overtone oscillator/wave
types, slope/fixed noise sequence, noise bands, filter/high-pass, clipping/bit
reduction, all envelopes and globals; public SYN and speech readers/renderers;
portable original source ownership for those readers; computed reference,
hostile-input and portable ownership fixtures. All listed task items for this
row remain open. No unverified recipe was substituted for either public format.
No licensing/technical impossibility or external-only deferral is claimed.

EXTERNAL-ONLY: lawful independent preset/reference corpus and listening after
those readers/models are implemented and tested.

## Integrator validation

Run serially in the integration checkout, with normal MSVC setup and an external
`TS_RS_EXPORT_DIR`:

- `cargo test -p windfall-project --test audio_regions`.
- `cargo test -p windfall-desktop --lib session::audio_editor::wav_regions`.
- `cargo test -p windfall-desktop --lib session::tests::audio_editor`.
- Existing native library/import/recording/file carrying, sample-playback WAV,
  song export and codec WAV marker suites, followed by full merged suites.
- Format the changed Rust files, run strict lint/typecheck and normal acceptance.
- Regenerate `AudioRegion`/containing bindings and WASM through the integrator
  workflow, then run the audio-editor frontend suites including the exact files
  above, with the prescribed four-worker limit, followed by frontend full gates.

## Least certain spots

1. Native bridge parsing and child-module methods have not been typechecked.
   The new source-global representation and exact-beat/ACID precedence need the
   independent byte fixtures and existing WAV interoperability tests first.
2. Source-to-view rounding for reversed tape pitch/offset, crop beat extents,
   metadata clipping and baked output endpoints need the new measured native
   fixtures. Do not loosen tolerances or assertions on integration failures.
3. Nested import batches append SetSampleRegions without allocating IDs. Verify
   exact created-ID order, source-version races and ordinary native import/history
   regressions. Timeline marker IDs remain retired after undo as required.
4. Frontend metadata uses the local audio-editor type; generated AudioRegion and
   the WASM document must be refreshed before the checked mounted fixture runs.
   Base UI Choice controls, dialog renewal and note editing remain unexecuted.
5. Formatting differences remain in all changed Rust files; permitted checks
   were syntax/format inspection only. No typecheck, tests, application workflow
   or audio render was executed. The two other assigned rows remain unimplemented.

HAND-OFF COMPLETE
