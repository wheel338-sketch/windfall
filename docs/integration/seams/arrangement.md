# Playlist arrangements integration seam

Owned delivery: `crates/windfall-project/src/arrangement.rs` and the isolated
`apps/desktop/src/features/playlist/arrangement/` folder. The parent declares
`pub mod arrangement` in lib.rs and owns model/command/history/playback wiring.
**Project integration is present, awaiting QA.** The playlist stores
`arrangementBook` with a serde default, so older files load an empty book.
Project checks validate structure, allocator IDs and every entity reference.
The playlist mounts the panel in ArrangementControls and dispatches checked
commands; playlist patches carry the saved book through undo/redo and reload.

## Stored value

`ArrangementBook` is an owned `Clone + Default` value with serde camelCase and
struct-level defaults. `{}` deserializes to an empty book. The parent adds its
embedding field with `#[serde(default)]`; optional serialization can use
`ArrangementBook::is_empty`. An absent embedding field has a container test.

- `arrangements: Vec<Arrangement>`: ordered alternatives. Each has a stable
  numeric id, a nonblank name (up to 256 UTF-8 bytes, no NUL), and ordered,
  duplicate-free `clips: Vec<ClipId>` and `tracks: Vec<PlaylistTrackId>` references.
- `active: Option<ArrangementId>`: exactly one existing alternative when nonempty;
  None for the legacy empty book. First add activates itself, later adds retain
  the active ID.
- `trackGroups: Vec<TrackGroup>`: stable numeric id and validated name.
- `groupParents: BTreeMap<TrackGroupId, TrackGroupId>`: child group to parent;
  absence means root. Self-parenting and indirect cycles are rejected.
- `trackParents: BTreeMap<PlaylistTrackId, TrackGroupId>`: track membership;
  absence means ungrouped. Removal promotes children and tracks to the removed
  group's parent or ungroups them at the root. It never deletes tracks.
- `clipGroups: Vec<ClipGroup>`: stable numeric id and at least two distinct clip
  IDs. A clip belongs to at most one group.
- `linkedTracks: BTreeMap<PlaylistTrackId, TrackKind>`: tagged
  `{ type: "instrument", channel }` or `{ type: "audio", source }`, using
  ChannelId and SampleId. No mixer routing is implied.

Field names above are JSON names; Rust uses snake_case. Arrangement/group IDs
are u32 aliases. Reference IDs reuse model newtypes and serialize as numbers.
Allocate metadata IDs from the parent's monotonic document allocator; helpers
have no allocator/global state. Current duplicate IDs fail; lifetime reuse must
be prevented by the caller (there are no tombstones).

## Command integration

All metadata mutations return `Result<ArrangementBook, ArrangementError>` from
`&self`. They check input, clone, edit and check the result. Prepare replacement
values before the first primitive edit; apply in one undo transaction with exact
redo IDs. Call these functions from the parent's command/lowering wiring:

- Alternatives: `add_arrangement(Arrangement)`, `rename_arrangement(id, name)`,
  `set_arrangement_references(id, clips, tracks)`, `switch_arrangement(id)` and
  `remove_arrangement(id)`. Last deletion returns LastArrangement. Removing
  the active one selects its next neighbour or the previous last. Switching
  changes the saved active ID only. **Playback still uses the single playlist
  until alternate layouts have their own clip entities.** No second clip payload
  store is introduced. No ticks, timeline or transport publication changes here.
- Track grouping: `add_track_group(TrackGroup, parent)`,
  `rename_track_group(id, name)`, `move_track_group(id, parent)`,
  `move_track_to_group(track, parent)` and `remove_track_group(id)`.
  None moves to root or ungroups a track. Parent checks track existence.
- Clip grouping: `add_clip_group(ClipGroup)` and `remove_clip_group(id)`.
  `expand_clip_selection(selected)` gives original selection order followed by
  remaining group members, without duplicates. Use for selection and deletion.
  `move_clips(clips, selected, tick_delta, track_delta, ordered_tracks)` returns
  the entire cloned clip pool sorted by Clip::sort_key. Every grouped member
  moves by the same delta or the whole move fails for missing IDs, zero length,
  song overflow/underflow or track bounds. Apply changed clips in one transaction.
- Links: `link_track(track, Some(kind), valid)` calls `Fn(TrackKind) -> bool`
  against actual channel/sample pools. Missing sources return MissingId.
  None removes a link without validating a source. Use `|_| true` only if the
  parent already validated it. Parent checks track existence and routing policy.
- Make unique: `make_unique(UniqueSource::Pattern(&pattern), allocate)` or
  `make_unique(UniqueSource::Audio(&sample), allocate)` returns
  `Result<(SourceReference, UniqueCopy), ArrangementError>`. The first item
  contains the new typed ID; the second is an owned pattern/sample asset to
  insert. Patterns get fresh root/note/meter/marker IDs and remapped note curves;
  channel references stay shared. Audio gets a fresh asset ID and retains its
  immutable file path. Physical file copying is separate. Zero/reused source or
  generated IDs fail. Use a staged, checked global allocator and commit its
  cursor only after success. Parent inserts the copy and redirects only the
  chosen clip in one undo transaction. Audio gain/routing/stretch remain intact.

`check()` validates metadata structure, names, duplicates, active ID and cycles.
Parent load/command checks additionally resolve all arrangement, membership,
clip-group and link references against its pools. Collapse/mute/solo propagation
and linked mixer routing remain integration work. Independent alternative
placements require distinct clip entities when layouts place the same source
differently; metadata stores references, not mutable clip payloads. Preserve
existing timeline-region/replacement guards in docs/TIMELINE-REGIONS.md.

## Connected panel

The playlist mounts ArrangementControls, whose ArrangementPanel reads the
saved project book and real track, clip-source, channel and sample names.
`onEdit` maps panel actions to the generated Command contract and dispatches
them through the project store. Add commands ignore advisory frontend IDs:
Rust stages IDs from `next_id`, validates the replacement before the first
primitive edit, then commits the allocator cursor after success. The panel
waits for the project patch and disables edits while dispatch is pending.
The local `onChange` mode remains available for isolated panel tests.

All alternatives/group/link mutations and MakeUnique are one undo transaction.
MakeUnique inserts a copied pattern/sample and redirects only the chosen clip;
pattern child IDs are fresh, audio routing/gain/stretch stay intact, and copied
audio assets can share an immutable file path. The file is not physically copied.
Commands that would leave dangling arrangement references are rejected and
rolled back, including deletion of referenced clips/tracks/channels/samples.
Generated ts-rs bindings now supply the stored book types.

Collapse, group mute/solo propagation, linked mixer routing, canvas grouped
selection/move/delete gestures, and applying alternate clip layouts are not
connected. Switching only changes the active ID. Existing timeline-region and
replacement guards remain in place.

## Parity scope

- win-playlist-arrangements: alternative ordered references and active selection.
- win-playlist-track-groups: nested metadata with cycle-safe moves and retained tracks.
- win-playlist-clip-groups: exclusive memberships, selection expansion and atomic movement.
- win-playlist-make-unique: owned source copies/fresh IDs with caller application.
- win-playlist-instrument-audio-tracks: typed links with validity callbacks.

These describe an implementation seam, not completed persisted parity rows. The
summaries also describe collapse/mute/solo, mixer routing and canvas gestures;
those need parent integration.

## Original isolated verification (before project integration)

Eight Rust unit tests pass in an external temporary Cargo harness that includes
the owned arrangement.rs by path and reexports the live windfall-project model.
That harness compiled the then-undeclared module without modifying lib.rs. Strict all-target
Clippy for that harness passes. Cases cover cycles, last deletion, pure switching,
group dissolution, grouped move bounds/atomic refusal, independent source copies,
fresh note/curve/timeline IDs, missing links and omitted-field/camelCase JSON.

Five focused Vitest tests pass in the new folder. The panel and its local model
pass a focused strict TypeScript check, ESLint and Prettier. Rustfmt passes for
arrangement.rs. The broad desktop typecheck encountered unrelated analysis,
channel-rack, mixer and simulator errors in the shared workspace; the focused
check excludes those feature entry points. No persistence or canvas integration
test is claimed.

## Project integration verification

`cargo check -p windfall-project` passes. Bindings regenerated from Git Bash
with `source scripts/msvc-env.sh && scripts/gen-bindings.sh` (314 files).
`tests/arrangement_integration.rs` covers one-step history for all commands,
actual file save/reload, exact redo IDs, allocator retirement/failure rollback,
missing-reference load refusal, legacy defaults, and chosen-clip source copies.
Panel command mapping and mounted controls have focused dispatch tests.
Integrated manual QA and parity certification remain pending.

The focused frontend suite passes 21 tests; the project library passes 34 tests
and the arrangement integration file passes 4 tests. Full project tests are
blocked by existing plugin-test syntax and missing patch fixture fields.
Desktop typechecking still encounters existing backend/simulator and unrelated
feature errors. The browser WASM simulator was not regenerated; the native
project command path is connected, and browser simulator commands need a later
artifact rebuild. Manual collapse/mute/solo, routing and alternate-layout QA
is not claimed.

## Independent layouts — 2026-10-10

This supersedes the statements above that alternate layouts are not
connected and that switching only changes an id.

`AddArrangementLayout { name, copy }` adds a timeline layout of its own and
makes it active. A project with no arrangement first keeps its present
timeline as "Arrangement 1", listing every clip and lane. The new layout
takes the active layout's lanes. With `copy`, every clip the active layout
lists is cloned as a new clip entity with the same lane, start, length,
offset, mute state and content, and a clip group whose members were copied
is cloned for the copies. Without it the layout has no clips. It is one
undo step and reports the arrangement id, then the copied clip ids.

A layout owns its clips, so a move, resize, mute or delete in one layout
cannot reach another. `SetArrangementReferences` can still list one clip in
two layouts on purpose. `RemoveArrangement` removes the clips that only the
removed layout listed; clips another layout lists, and clips no layout ever
listed, stay.

`ArrangementBook::shows(clip)` is the one rule for "does the active layout
show this clip", used by consolidation. The engine and the canvas apply the
same rule in `arrangement_playlist` and `arrangementPlaylist`.

The popover has **New empty layout** and **Duplicate layout**. The older
**Add arrangement** still lists the same clips again and is kept for that.
An arrangement with no clips now shows its lanes.

## Linked tracks — 2026-10-10

`TrackKind` has a third kind, `{ type: "audioTrack", mixer }`.

- An **instrument track** (`instrument`) plays only its channel of the
  patterns placed on it. The engine compiles this into `PlanClip.only`; a
  link whose channel is gone plays nothing on that lane. Audio clips on the
  lane are not affected.
- An **audio track** (`audioTrack`) routes every audio clip on it to its
  mixer track. The document enforces it inside the command's own
  transaction (`settle_linked_tracks`): on add, on a move onto the lane,
  and when the lane is linked. A clip moved off the lane keeps the routing
  it has. Deleting the mixer track drops the link.
- `audio` (a sample) is still a label and routes nothing.

`AddInstrumentTrack` and `AddAudioTrack` make the channel or mixer track
and the linked lane in one undo step. The header menu has **Link track**;
pressing a linked header selects its channel and mixer track.

MIDI export of a song does not apply the instrument-track filter.
