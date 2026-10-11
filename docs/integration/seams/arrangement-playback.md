# Windfall arrangement playback integration seam

`ArrangementBook` still stores references to the single playlist clip and track
pool. Clip positions are not copied per arrangement. Switching arrangements
changes the active id; it does not rewrite clip starts, lengths, offsets or tracks.

## Plan and view

`crates/windfall-engine/src/plan.rs` resolves the active arrangement while building
the plan on the control side. Pattern, audio and automation compilation all receive
the filtered playlist. No arrangement lookup, allocation, locking or I/O is added
to the audio callback.

`apps/desktop/src/features/playlist/arrangement-view.ts` derives the playlist view
from the same reference rules. Track headers, canvas clips and edit destinations
share `track-rows.ts`; arrangement track order takes precedence over grouping,
while group collapse still hides its members. Edit destinations retain their
indices in the stored track pool. Arrangement changes rebuild the canvas and its
automation holds without moving any saved clips.

- No arrangements or no active id retains every track and clip.
- An active arrangement includes only its clip ids on its track ids. Tracks are
  displayed in the arrangement's reference order. A listed clip on an excluded
  track is hidden and silent.
- Either an empty clip list or an empty track list yields no tracks or clips.
  Empty lists never mean everything.
- Playlist mute and solo apply after filtering. A solo track outside the active
  arrangement cannot silence its tracks. Audio crossfades use only the remaining
  audible clips. Automation uses the arrangement's track order for overlap priority.
- The plan and view song end use included clips, retaining muted clips when
  computing the active timeline length.

## Verification

From the repository root in Git Bash:

```sh
source scripts/msvc-env.sh
TS_RS_EXPORT_DIR=target/ts-rs-discard cargo test -p windfall-engine --lib arrangement_
TS_RS_EXPORT_DIR=target/ts-rs-discard cargo test -p windfall-engine --test arrangement_playback --test playlist_track_solo --test playlist_crossfade
```

From `apps/desktop`:

```sh
pnpm test src/features/playlist/arrangement-playback.test.tsx src/features/playlist/track-group-view.test.tsx src/features/playlist/track-solo.test.tsx
```

Plan unit tests cover the empty book fallback, single-clip selection for pattern,
audio and automation, clips on excluded tracks, empty reference lists, no active
id, mute, hidden solo tracks and crossfades after filtering. Desktop tests cover
ordered headers and canvas filtering, switching without position changes, empty
arrangements, legacy fallback, document row indices and group collapse.

The public-renderer command passed all 9 tests; the desktop command passed all
17 tests. The plan unit-test command is currently blocked by unrelated existing
test compilation errors in `project_preparation.rs`, `device.rs` and `render.rs`
(missing fixture fields and `StemError`/`RenderError` mismatches).

## Empty arrangements — 2026-10-10

An active arrangement that lists lanes and no clips now shows those lanes
in the playlist, so that the first clip of a new layout has somewhere to
go. It still plays nothing. An arrangement that lists no lanes shows
nothing, as before. The engine is unchanged.
