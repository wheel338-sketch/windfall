# Playlist clip slicer foundation

The playable instrument extension is documented in
[the current sample-instrument seam](integration/seams/granular.md). Slice Map and
Slice Deck select original imported sources, detect transients/beat grid, edit
regions and generate key-mapped notes from the instrument panel. Slice Deck adds
per-slice controls and source B; Grain Cloud and Wave Ride share the immutable
source path with independent scheduling/curve DSP. Their written, unexecuted
acceptance gates are in [the worker hand-off](qa/2026-10-11-slices-grains.md).
Playlist verification below does not establish acceptance of these new paths.

Select one audio clip in the playlist. In its audio settings strip, choose **Slice clip**. Choose a song-aligned beat grid (sixteenth, eighth, quarter, half, or four quarter notes), or **Transients** with a sensitivity from 0 to 100%. Press **Analyze markers**, inspect the waveform and cut list, disable unwanted markers, then press **Apply slices**. Changing the detector settings requires another analysis. Analysis and marker review do not edit the project.

Apply replaces the selected clip with adjacent, positive-length clips using the same sample asset and file. Each slice keeps the playlist track, mixer routing, gain, pan, mute, reverse, and tape pitch. Its start and source offset advance by the same number of ticks, so a trimmed or reversed clip keeps its musical placement and playback direction. Slices remain independently selectable/editable ordinary playlist clips. **Slice audio clip** is one undo step; redo restores the same slice IDs. Save/reopen needs no new format or rendered files. The original audio file is never written.

This is a Phase-3 clip slicing foundation. It is not a playable instrument, MIDI slice mapper, sampler pad bank, proprietary-equivalent instrument, tempo estimator, or stretch renderer. Markers are review-only until Apply and are not saved as a separate marker track. The source-linked slices themselves are saved.

## Supported settings and limits

- Trim, source offset, reverse, tape pitch, gain, pan, mute, and routing are preserved.
- Nonzero clip fades, spectral stretch, nonzero project swing, and projects with tempo automation are explicitly rejected before any mutation. Offset-only slices cannot yet preserve their complete envelopes or time warp. Remove those settings before slicing.
- Clips deliberately extending past available audio, missing/empty sources, non-finite audio, invalid sensitivity/grid, and out-of-range timeline/source offsets are rejected. The final fraction of a tick from the importer's rounded-up natural length is allowed.
- At most 2,048 interior markers and 32 million visible source frames are analyzed. A coarse grid or shorter trim can reduce work. Duplicate, unordered, boundary, unauthorized, or empty cut lists are refused; no partial edit occurs.
- Transient detection measures channel RMS in roughly 5 ms playback windows and thresholds positive rises relative to the strongest attack. Sensitivity lowers that threshold; a 30 ms refractory interval avoids adjacent detections from one attack. Opposing stereo polarities do not cancel. Markers round to the project's 960-PPQ tick resolution. This simple energy detector can miss gradual attacks or include noise, so review is required.
- The waveform is an absolute-peak overview of the visible region in playback order. It does not show the exact signed signal. Normal engine edge declicking (3 ms) applies to every slice, so slicing is not sample-identical to continuous playback around boundaries.
- The browser backend's library and audio are simulated fixtures. Its dialog labels this explicitly. It runs the same Rust detector in a dedicated Web Worker, with a separate WASM instance; headless jsdom tests call the same WASM export directly. Native analysis uses decoded file audio. Browser QA is not evidence of native hardware playback.

## Implementation and ownership

`windfall-project::slicer` owns detector options/results, support validation, and construction of the single checked `Command::Batch`. It changes no realtime processing code. `windfall-ipc::SliceReview` carries a token, clip ID, length, and analysis. Native IPC commands are `slice_analyze`, `slice_apply`, and `slice_discard`; the browser ABI exports `slice_analyze` and `slice_command`. UI controls, overview, and worker live in `features/slicer`.

Native analysis snapshots immutable audio and clip/settings under recording → State locks, then releases both before scanning. Apply clones the document/pool, validates the batch and compiles its playback plan outside State. Publication reacquires recording → State and checks document generation, edit count (including undo/redo), latest review token, retained source identity, and the compiled pool's identities. A reload, document replacement, newer analysis, intervening edit, or recording started during the work rejects obsolete results. Each session retains at most one review. Discard releases the review; a stale discard cannot erase a newer token. No new path runs on the audio callback, captures active plugin state, or changes controller-side retirement.

The UI also checks its document generation and mounted lifetime before accepting analysis or Apply replies. Reused document revisions and clip IDs after New/Open cannot attach an old patch or selection to the replacement project.

Integration must rebuild `windfall_sim.wasm` and its manifest from the combined source tree. This feature branch uses a local rebuild for verification but deliberately does not commit its branch-only generated WASM. No generated TypeScript bindings are hand-edited; the new wire types are scoped in `features/slicer/types.ts`, exercised through actual Rust/WASM replies.

## Verification

Targeted detector tests cover deterministic song alignment, weak/strong stereo attacks, silence, reverse/trim/pitch mapping, positive adjacent slices, retained settings/source references, undo/redo/file round trips, and invalid inputs. Native headless session tests use synthetic WAV audio/capture and forced worker pauses to check recording exclusion, edits during analysis/compilation, source replacement, stale/latest/discarded tokens, byte-identical original files, and save/reopen. Frontend tests use the real locally rebuilt WASM to check the review workflow, marker toggling, settings/revision invalidation, post-close results, backend staleness, and existing playlist audio UI behavior.

Actual command results and browser checks are recorded in [slicer/verification-2026-10-07.md](slicer/verification-2026-10-07.md). No physical audio device, native WebView UI, macOS, or Linux verification is claimed.
