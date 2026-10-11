# Selected-note tools

Select notes in one piano-roll channel/pattern, then choose **Note tools** in
the toolbar, a selected-note context-menu action, or an action in the command
palette. Ctrl+Q opens quantize starts; Alt+Q opens quantize ends. Use Select all
first when the intended target is the whole lane. Empty selections do not fall
back to all notes. Octave-transpose shortcuts also require a selection.

Options and Cancel do not edit the document. There is no live DSP preview.
Apply submits one shared Rust `TransformNotes` command through the ordinary
document/engine dispatch path. All note changes and any necessary pattern
extension are one undo entry. A transformation that changes nothing adds no
history entry and preserves redo and dirty state. Existing notes retain their
IDs as each tool describes; newly created pieces/hits receive fresh IDs and
join the selection. Undo prunes missing selected IDs; redo does not automatically
reselect those IDs.

## Tools and defaults

| Tool | Behavior and default |
| --- | --- |
| Quantize | Starts or ends, current snap grid (240 ticks if snap is off), 100% strength, Straight. Starts move with lengths preserved; ends move with starts preserved. |
| Legato | End each selected onset/chord at the next distinct selected onset, extending or trimming lengths. The last selected chord keeps its lengths. |
| Staccato | Multiply lengths by 50%, rounding to whole ticks, with a minimum of one tick. UI range 1–100%. |
| Portamento | Legato lengths plus an overlap (default 0 ticks), and every note after the first selected onset becomes a portamento note with the chosen glide (default 240 ticks). The first onset and slide notes keep their articulation. Added 2026-10-10; written, not yet run in the UI. |
| Chop | Split on absolute pattern grid boundaries, defaulting to snap/240 ticks. Retain partial first/last segments and all pitch, velocity and pan values. The first piece keeps the original ID. |
| Custom Chop | Repeat user-authored boundaries from Origin with optional per-step gate/velocity accents. Keep partial edges. |
| Arpeggiate | Rewrite exact-onset chords into repeated ascending, descending or alternating hits, with rate/gate/octave/duration controls. |
| Flam | Add a close grace hit explicitly before or after each original, with interval and relative velocity controls. |
| Rhythm reshaper | Remove, add or shift selected notes in a repeating onset-cell phase, with origin/grid/period/offset controls. |
| Glue | Union touching/overlapping selected notes only when pitch, velocity and pan are identical. Keep the earliest start's ID, breaking equal-start ties by lower ID. Incompatible or unselected notes remain separate. |
| Strum | Group exactly simultaneous selected onsets, sort low-to-high pitch then ID, delay each successive note by 30 ticks, and reduce velocity by 5 percentage points per note. High-to-low reverses pitch order and retains the ID tie-break. Lengths are preserved. |
| Flip time | Reflect starts/ends within the selection's earliest start and latest end. Lengths, keys, velocities and pans are preserved. |
| Flip pitch | Reflect pitch within the selected minimum/maximum MIDI keys. All other properties are preserved. |
| Limit / transpose | Transpose by 0 semitones, then clamp to MIDI keys 48–84 by default. Optional octave folding chooses the nearest octave-equivalent pitch within the range; ties choose the lower key. A range with no representative of that pitch class falls back to clamping. |
| Scale velocities | Multiply selected velocities by 80% by default; UI range 0–400%. Keep relative differences until clamping at full velocity. Other note properties are preserved. |

Time uses 960 ticks per quarter note. All grids are integer tick counts from
1 to 245,760. MIDI keys are 0–127, velocities 0–1, and pan −1–1. Transpose is
an integer from −127 to 127 semitones. Strum spacing is 0–245,760 ticks and
velocity step is −100–100 percentage points per note.

See [PIANO-RHYTHM](PIANO-RHYTHM.md) for the exact rhythmic policies, examples,
field validation, ID rules, edge behavior and repeat-Apply semantics of the four
rhythm workflows. They use the same atomic selected-note command and guards.

## Original grooves and rounding

These small repeating rhythms were written for Windfall. They do not contain
proprietary groove files or template names.

- Straight: no offset.
- Swing: delay odd grid divisions by one sixth of the grid.
- Late pairs: delay odd divisions by one quarter of the grid.
- Push four: advance division 3 in each group of four by one sixth of the grid.

Division zero is pattern tick zero. Fractional groove offsets truncate to
whole ticks. Quantize chooses the nearest groove line, breaking equal-distance
ties toward the later line. Strength interpolates between the old tick and
the target, then rounds half ticks upward. Start quantize clamps to leave the
original length inside the maximum pattern; end quantize clamps between one
tick after the start and the maximum pattern end. Staccato also rounds half
ticks upward. Strum rejects the entire command if any delayed note would end
past the pattern limit.

## Selection, transactions and limits

The dialog captures selected notes, pattern/channel IDs, and the document
revision. A changed document, channel/pattern, selection, closed request, or
project replacement invalidates that review. Rust checks every captured note
against the current lane before changing anything, so missing/stale/wrong-lane
IDs fail atomically. Identical repeated IDs normalize to one note; conflicting
snapshots of one ID fail. Input order does not affect the result. Output lanes
use the existing start/key/ID ordering.

Selections and tool output are bounded to 16,384 notes. A finer chop that would
exceed this limit fails before allocating IDs or editing notes. Nonfinite
numeric parameters, invalid ranges, zero-length notes and selected notes ending
outside 245,760 ticks are rejected. This is deliberately stricter than loading
legacy notes beyond that limit: move/trim those notes into range with the
existing editor before applying these tools. No persisted fields, file-format
change or new dependency were introduced.

The command uses the existing transaction rollback and undo edits. Only the
pattern is touched; plugin bindings/ownership/state, channel engine ownership,
MIDI hardware and playlist editing are outside this change. No filesystem or
DSP work was added to the command or audio callback. Native dispatch retains
its recording-before-document lock order.

## Parity scope for integration

Implemented and checked within the assigned selected-note scope:
`win-piano-quantize`, `win-piano-chop`, `win-piano-glue`, `win-piano-strum`,
`win-piano-limit`, `win-piano-flip`, and `win-piano-scale-levels`.
`win-piano-articulate` has working legato/staccato length tools. Portamento
phrasing was added on 2026-10-10 as the Portamento tool above, on top of the
slide/portamento note model; its acceptance
(`crates/windfall-project/tests/piano_articulation_lfo_qa.rs`) is written and
has not been run, so the row stays in progress until it has. Chop now also supports bounded user-authored slicing patterns;
Arpeggiate, Flam and Rhythm reshaper are described in PIANO-RHYTHM. Scale levels
affects velocity only. Randomization and preset import/export remain outside
this workflow. The separately reviewed scale/stamp workflow is described in
[PIANO-SCALES](PIANO-SCALES.md).

The parent owns parity matrix/README updates and shared generated artifacts.
After merging, regenerate TypeScript with `scripts/gen-bindings.sh` and WASM
with `scripts/build-sim.sh`. They were generated locally for verification and
are intentionally excluded from this implementation commit.

## Checks performed on Windows

- Final native checks used the fresh, worktree-local
  `target/piano-tools-verification` Cargo directory and a temporary
  `TS_RS_EXPORT_DIR`; no copied cross-worktree build metadata was reused.
- Native `cargo test -p windfall-project`: passed, including existing command,
  file, plugin persistence and property tests (281 tests). Final focused
  `cargo test -p windfall-project --test piano_tools`: 12 passed. These cover
  all tools, invalid parameters/notes, deterministic duplicate/order behavior,
  groove rounding/boundaries, stale/wrong-lane selection, ID exhaustion,
  identity/redo behavior, atomic pattern extension, undo/redo, and real file
  save/load for every transformation.
- `cargo test -p windfall-sim`: 12 passed. The browser mock and UI tests use the
  real Rust WASM document rather than a second TypeScript implementation.
- `cargo check -p windfall-desktop --lib`: passed with the Windows MSVC
  environment. `scripts/build-sim.sh` and `node scripts/check-sim.mjs`: passed
  during implementation. The final Rust WASM was also rebuilt in the fresh
  `target/piano-tools-wasm-verification` directory with the script's size and
  source-path-remapping settings; `node scripts/check-sim.mjs` passed.
- `cargo clippy -p windfall-project --all-targets -- -D warnings` and
  `cargo fmt --all -- --check`: passed.
- `pnpm typecheck`, `pnpm lint`, and `pnpm build`: passed. Vite reports the
  existing large-bundle warning.
- Full UI `pnpm test --maxWorkers=4`: 130 files / 2,117 tests passed. After
  adding the final selection-review regression, the focused piano-roll and
  exhaustive WASM-command suite passed 6 files / 212 tests. The new tool suite
  covers apply/undo/redo, persistence, unrelated properties, generated-command
  coverage, native/WASM quantize expectations, dialog Apply/Cancel/Escape,
  invalid input and revision/lane/same-size-selection/replacement guards.
  After the final glue regression and fresh WASM build, the tool/dialog and
  exhaustive WASM-command tests passed 2 files / 75 tests, with typecheck and
  lint passing again.
- T3 collaborative Chromium preview at 1280×800: inspected the accessible
  dialog, changed strength/groove without a revision/history edit, verified
  invalid grid disables Apply and Cancel is inert, chopped four notes to eight
  in one history entry, exercised undo/redo, then selected all pieces and
  glued them back to the original four with one history entry. A clean reload
  with the final WASM accepted a fractional 50.05% quantize strength and closed
  the dialog without adding history for an already-aligned selection.

Replacing WASM during Vite hot reload produced duplicate mixer action
registration and an empty piano-roll context in that development session.
A clean page reload recovered both panels; no mixer or hot-reload code was
changed here.

Native project behavior was tested on Windows. A packaged Tauri interaction,
macOS/Linux execution, physical MIDI hardware and sound-card playback were not
verified by this task.
