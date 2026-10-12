# Audio editor

Select one playlist audio clip and choose **Audio editor** in the clip settings
strip. The native editor shows a waveform of that clip's current window. Drag
either direction to select a range, or type exact frame endpoints. Start is
inclusive; end is exclusive. **Select all** restores the whole window.

Each operation commits immediately as one undo step, selects the resulting
clip and renews the native preview while keeping the editor open. On errors,
the selection remains available. A stale project/source error requires closing
and reopening; no old result is attached to a new document.

- **Trim to selection** keeps only that range and replaces the old clip. Its
  timeline start moves to the selected position, rounded to the nearest tick.
- **Extract selection** keeps the old clip and adds a new clip after its end,
  on the same playlist and mixer tracks.
- **Normalize** applies one linked gain to the selected channels, targeting
  −1 dBFS peak. Samples outside the selection do not change. Silent selections
  are refused.
- **Reverse selection** reverses frames, preserving stereo channel pairing.
- **Fade in/out selection** multiplies the range by playback's equal-power
  sine fade, including silence/full-level endpoints. A one-frame fade silences
  that frame.
- **Silence selection** zeros the range without changing duration.
- **Cut selection** removes the range and joins the remainder. Cutting the
  entire clip is refused; use the playlist's ordinary Delete action instead.

## Cleaning and processing tools

Added 2026-10-10. The dialog groups these under **Clean up** and **Process**,
below the original edits. Each acts on the selection, commits as one undo
step and writes a derived WAV like the edits above. Samples outside the
selection are bit-identical afterwards; the tools that change length move,
but never alter, the audio after the selection. A control outside its range
disables its button; a request the editor refuses anyway leaves no file and no
history entry.

Clean up:

- **Remove DC offset** subtracts each channel's own mean over the selection.
  The channels are not linked, because converter offsets differ per channel.
- **De-click** (strength 0–100 %) finds short impulsive outliers with a
  32nd-order autoregressive model fitted per 2048-sample block and replaces
  them by least-squares interpolation from the audio around them, at most 128
  samples in one cluster. Prediction errors below −80 dBFS are never treated
  as clicks. Samples that are not detected keep their exact value.
- **Reduce noise** (0.1–48 dB) is an overlap-add Wiener gain against a
  stationary noise spectrum, with decision-directed smoothing and a floor at
  the chosen reduction, so noise is turned down evenly rather than gated. One
  gain per bin is shared by both channels. **Use selection as noise profile**
  remembers a noise-only range to learn the spectrum from; without a profile
  the noise floor is estimated from the selection itself as a low quantile
  over time with a median across neighbouring bins that rejects steady tones.
  Where the selection ends inside the clip the result is crossfaded into the
  untouched audio over a quarter window. A silent profile, or one outside the
  clip, is refused.
- **Trim silence** (threshold −120 to −6 dBFS) removes the selection's leading
  and trailing frames below the threshold. The clip's start follows what was
  removed from its front.

Process:

- **Apply gain** (−96 to +96 dB, not 0) is one linked gain. A boost that would
  pass full scale is refused with the largest boost that fits; it is never
  clipped.
- **Normalize to target** (−60 to 0 dBFS, peak or RMS) uses one linked gain.
  The original **Normalize** button stays peak at −1 dBFS.
- **Fade in/out with shape**: equal power (playback's sine law), linear, or
  S-curve (raised cosine). The original fade buttons stay equal power.
- **Swap channels**, **Mono sum** (the average of left and right to both) and
  **Invert phase** (both, left or right).
- **Stretch and pitch**: length 25–400 % and pitch ±24 semitones,
  independently, through the same spectral preparation a stretched playlist
  clip uses. Only the selection is stretched; the clip gets longer or shorter
  by the difference.
- **Insert silence** (milliseconds, rounded to frames) before the selection's
  first frame.

Clip length follows the audio: the same frame count keeps the tick length; a
shorter result rounds up and is capped at the old length; a longer result
takes the fewest ticks whose reopened view holds every frame, and is refused
if it would pass the end of the song. Several edits in a row name the derived
sample after the latest edit, not after the whole chain.

Request compatibility: the eight original operations keep their wire names.
Operations with parameters travel as an object with one key, and every
original processing parameter has a default, so `{"declick": {}}` parses.
The advanced operations added in the second pass require their explicit fields.

Status of this section: written, not executed. The Rust and TypeScript were
finished under an instruction not to build or test, so none of the tests
below has been run. No measured figures are claimed here; the bounds asserted
by the DSP tests are in the test sources.

- `crates/windfall-engine/src/audio_edit/clean.rs` (`audio_clean_*`, nine
  tests): DC removal, exact silence bounds, de-click on a sine and on a noisy
  chord, clean audio and the outside untouched, noise reduction SNR in both
  modes, reduction amount, automatic estimate against a known floor and a
  tone, refusals.
- `crates/windfall-engine/src/audio_edit/process.rs` (`audio_process_*`, five
  tests): gain exactness and refusal to clip, peak and RMS targets, fade shape
  endpoints and midpoints, channel tools, stretch duration and pitch.
- `crates/windfall-engine/src/audio_edit.rs` (`audio_edit_*`, three tests):
  trim and insert boundaries, selection stretch splice lengths, every
  operation deterministic and preserving the outside.
- `crates/windfall-ipc/src/audio_edit.rs` (two tests): old requests still
  parse; parameters default and use camel case.
- `apps/desktop/src-tauri/src/session/tests/audio_editor.rs` (four tests added):
  tools write what the engine computes as one undo step each; length-changing
  tools move start and length; refused tools leave no file and no history;
  several edits in a row undo one at a time.
- `apps/desktop/src/features/audio-editor/tools.test.ts` and
  `tools-panel.test.tsx`: request shapes and ranges, the noise profile, a
  disabled tool for an out-of-range parameter or an invalid selection, a
  native refusal.

## Second-pass editor tools

Added 2026-10-12; **written, not executed**. The first-wave execution evidence
below predates these additions.

- **Audio editor recorder** in the mixer Add effect menu inserts a transparent
  Send tap slot. Its effect editor has Record, Stop and edit, and Discard.
  The native logger captures immediately after that slot and before the next
  effect/fader. Stop attaches one WAV/clip as one undo step and opens it for
  editing. Insert the slot between the effects whose intermediate signal you
  want. Duration/memory limits follow the audio logger. Changing logger sources
  starts a new history; another slot recorder must finish before this one starts.
  New/Open disarms slot sources because effect ids are document-local.
- **Show spectrum** displays a Hann-windowed 128-column, 64-bin spectrogram,
  linear frequency from DC to Nyquist, brightness from −96 to 0 dBFS. It uses
  the louder channel's magnitude so opposite-phase stereo remains visible.
  The waveform and spectrum share the same exact frame-selection controls.
- **Regions/markers** add named source-frame metadata to the sample asset.
  Equal endpoints are markers. Select or remove entries in the editor;
  region writes are one guarded undo step and survive save/reopen. Display
  anchors follow clip offset, reverse and tape/spectral timing. Derived edits
  carry clipped/transformed anchors into their own sample. A region that spans
  a partial reverse remains a contiguous bounding interval.
- **Equalize selection** applies smooth three-band FFT EQ with transitions at
  250 Hz/4 kHz and ±24 dB controls. A boost that clips is refused.
- **Blur selection** smooths spectral magnitudes over neighbouring bins while
  retaining phase, with a 1–100 % amount.
- **Convolve selection** uses a captured range from this view as an impulse,
  up to 32768 frames, with a wet mix. It performs linear convolution per channel
  and crops the tail at the selection boundary. No proprietary IR is supplied.
- **Repair clipping** reconstructs short plateaus using Hermite interpolation.
  It requires two clean frames on each side and refuses plateaus over 256 frames;
  it cannot recover lost original information exactly.
- **Tune loop seam** crossfades the selected tail toward its reversed head so
  the last sample meets the first. The chosen crossfade uses at most half the
  selection and keeps its duration.
- **Split and strip silence** finds linked-stereo quiet runs, replaces the clip
  with the retained sections at their original song positions, and leaves audio
  outside the selection in place. The derived WAV retains source frame offsets;
  playlist placements/offsets use whole ticks and refuse overlapping splits.
- **Stretch and pitch** now exposes Fast/Standard/High quality and formant
  preservation for the selection.
- **Drag selection to sampler or playlist** creates one Float32 WAV and attaches
  it in one native transaction. Sampler assignment resets trim/reverse/loop/
  stretch/tune to avoid applying them twice; other channel controls remain.
  Playlist drops use the destination lane and snapped start. The original clip
  stays in place. Expired tokens and late replies after project replacement refuse.

### Script worker hookup

**Run script** is connected to Windfall's existing bounded postfix interpreter.
The source, parameter vector and active token/range travel through blocking IPC
`audio_script_preview`. It retains one selection result and returns its actual
waveform without a derived file/history entry. **Audition script result** uses
`audio_script_audition`; **Apply script result** uses `audio_script_apply` and
attaches the cached selection through the same guarded publication path as
`Session::audio_editor_run_script_selection`. Apply never reruns source and renews
the ordinary editor token on its new clip. Cancel uses the shared nonblocking
`script_cancel` ticket and is checked again immediately before document mutation.
Late/stale/cancelled work cannot publish a derived asset.

The audio adapter adds selection-only `gain EXPRESSION`, `reverse` and
`normalize AMPLITUDE` statements to the same compiler/arithmetic evaluator. It
accepts no loops, notes, tempo, map, feedback or host handles. Frame position is
`value` (0..1), frame index is `index`; parameters retain the existing checked
declaration/range rules. The panel ships original gain-ramp, reverse, linked-peak
normalization, linear fade-in and squared fade-out examples. See `SCRIPTING.md`.

There is one script worker shared with piano scripting, a cooperative five-second
interpreter deadline, 100 million counted instructions/sample visits and the
existing 16-KiB source/512-op/32-stack/16-parameter bounds. Input, selected scratch
and output each obey the editor's two-minute/64-MiB cap. Every pass checks cancel,
including validation, peak scans and reverse. File IO/engine preparation remain
outside the interpreter's execution deadline. Audio outside the selection stays
unchanged, source WAVs remain untouched and the derived result saves/reopens
through normal project storage. Browser mode reports the native capability error.
Sending `"script"` to the built-in apply command still refuses direct execution.

Round-four source and deterministic fixtures are written, not executed. Integrator
owns compilation, bindings/WASM regeneration and native-window acceptance.

## What is rendered

The edit input is a stereo clip view at the original source's sample rate,
including its offset, reverse, tape pitch or prepared spectral stretch/pitch,
gain, pan and explicit equal-power fades. Mono sources become stereo. Source
exhaustion fills the remaining clip window with silence. The editor shares the
engine's Hermite region reader, spectral sample preparation, pan law and fade
function. Its waveform uses min/max across channels so opposite-phase stereo
remains visible.

Mixer effects, sends, automation and the engine's transient transport/de-click
ramps are excluded. Ordinary playback still applies its de-click ramps to the
resulting clip. Cut/reverse selection boundaries are hard edits in the WAV;
apply selection fades to soften them when needed. Edited clips retain their
playlist track, mixer route and mute flag. Their baked offset/reverse/pitch/
stretch/gain/pan/fades reset to unity/defaults to avoid applying them twice.

Frame selections are exact in the derived WAV. The playlist stores whole ticks,
so trim placement rounds to the nearest tick and shortened clip length rounds
up, capped at the original length. Normalize/reverse/fade/silence retain the
original tick length, even when a frame boundary spans a fractional tick.

## Assets, undo and persistence

Source WAVs, imported files and recordings are never written. Every edit writes
a uniquely reserved Float32 WAV in the app settings folder's
`recordings/Audio edits` directory. It becomes a normal project sample with an
external absolute path. A single batch adds that sample and adds the resulting
clip; replacement operations remove the old clip in that same batch. The
original sample asset remains in the project. Optional source-frame regions
are omitted when empty; `SetSampleRegions` is the metadata history command.

Undo restores the old clip and removes the derived sample/clip from the
document; redo restores them. Successful derived files remain on disk for
history and saved documents. Save/reopen and a fresh session load them through
the ordinary sample loader. Refused work removes its reserved output file.
The sampler inspector's **Clean unused derived audio** action collects outputs
created in this session only when current/undo/redo roots do not reference them.
Saved/backup/imported sources are pinned. Unknown files and older-session files
are retained because the app cannot establish that other saved projects no
longer need them. Refused outputs are removed immediately.

These are persistent app-owned assets, **not a portable project bundle**.
Moving a project to another machine requires copying its external audio assets
and relinking them through existing workflows. This first editor does not
change the project's general sample packaging behavior.

## Threading and limits

IPC `audio_editor_open`, `audio_editor_apply`, `audio_editor_regions`,
`audio_editor_transfer` and `audio_editor_discard` run on
blocking workers. A dedicated editor mutex serializes jobs and retains at most
one immutable view/token. It is never used by the audio callback. Source/view/
spectral-output buffers are each limited to two minutes and 64 MiB; oversized,
empty, non-finite, unloaded or more-than-stereo inputs are refused before slow
preparation. Spectral scratch memory and the engine's existing bounded variant
cache are additional to these per-buffer caps.

The session snapshots with recording exclusion before the document lock,
releases both for DSP, WAV writes, decoding and engine preparation, then
reacquires recording exclusion followed by state to install. It checks document
generation, edit count, the original clip, source buffer identity and every
compiled pool source before dispatch. A take active at either snapshot or
installation refuses the edit. Successful installation uses the existing
prepared-project publication and controller retirement path. No active plugin
state is captured and VST3 is not enabled by this feature.

Projects containing tempo automation are explicitly refused for now, even if
the tempo lane is currently unused/muted. This prevents a constant-tempo edit
from silently changing a clip's automated timing.

## Browser and verification

The browser backend refuses editor open/apply with an explicit desktop-required
message. It neither fabricates decoded audio nor edits the simulated project.
UI fixtures provide native preview responses solely to test selection,
validation, busy/error/cleanup and IPC behavior; actual audio math and file
persistence are covered by Rust fixtures.

Verified on 2026-10-07 in this Windows worktree: eight native editor session
tests, all 88 engine library tests (four editor DSP tests included), and 26
focused UI/playlist tests passed. Rust fmt and clippy with warnings denied,
frontend typecheck and lint were also run. No generated bindings/WASM were
changed.

T3 browser evidence:

- [Browser refusal](evidence/audio-editor/browser-refusal.png): the real
  browser backend refuses decoding/WAV creation, with project history unchanged
  after opening the editor.
- [Editor layout fixture](evidence/audio-editor/ui-fixture.png): a labeled
  four-frame stereo preview matching the Rust fixture's min/max values. This
  tests UI rendering only; native file operations remain unsupported in the
  browser.
- [Frame selection at 800 × 600 CSS pixels](evidence/audio-editor/ui-selection-800x600.png):
  typed frame range `[1, 3)`, all operations visible, no horizontal overflow.

Targeted checks run with task-specific `TS_RS_EXPORT_DIR`, using Git Bash and
`scripts/msvc-env.sh` on Windows. Evidence is in the audio-edit core tests,
`session/tests/audio_editor.rs`, and `features/audio-editor/*.test.*`:
exact mono/stereo selection operations, source bytes unchanged, linked
normalization, playback transforms and spectral duration, atomic undo/redo,
save/fresh reopen, token/range/write/load errors, stale edit/source/document
races, and recording starting during preparation. The T3 preview checks the
playlist entry and honest browser refusal. These checks use synthetic/fake
audio input; no physical hardware or macOS/Linux verification is claimed.

## Parent integration

Second-pass shared seams also include the optional regions model/command,
logger effect-source contract and chain taps, Send tap recorder editor/menu,
sampler/playlist drag handlers and the dialog's optional backdrop. Preserve
parallel slicer/inspector and command-list additions when integrating.
The feature-local TypeScript IPC contract matches `windfall-ipc/audio_edit.rs`;
the parent owns the final combined ts-rs bindings and browser WASM regeneration.
No generated bindings/WASM, parity files or publication are part of this branch.
