# Playlist track export

Windfall's export dialog can write one file for each playlist track. It is
a third kind of export beside the mix and the mixer stems, and it answers a
different question: a mixer stem follows a mixer track, a playlist stem
follows the music placed on one lane of the timeline.

## What a file holds

The song with only that lane's pattern and audio clips placed, heard at the
output. Everything between a clip and the output stays in place: the
channels' and clips' mixer tracks, their sends, the buses they reach, and
the master's effects and fader. It is what the lane sounds like soloed.

- **Shared bus and master processing is included.** Where that path is
  linear, the files of all lanes add up to the mix.
- **Level-dependent processing reacts to each lane alone.** A compressor
  or limiter on a bus or the master works on what it is given, so those
  files do not add up to the mix. A sidechain detector hears only the lane
  being rendered: a kick on another lane does not duck this one.
- **Automation plays in every file**, wherever its clip sits, so a fader
  ride or a tempo change is the same in each.
- **A print belongs to its lane.** A Direct-output clip, such as a
  consolidated track, is in the file of the lane it sits on.
- **Mute and solo are as the project has them.** A lane the mix does not
  play has nothing to give: it is left out of "all tracks", and asking for
  it by id is refused.
- **Voices are the lane's own.** Within a file a note is never cut by a
  note of another lane, which can differ from the mix where lanes share a
  channel with a cut group or a voice limit.
- With an active arrangement, the lanes and clips are that arrangement's.
- Every file starts where the song starts and is as long as the mix.

Two lanes may play the same pattern, channel or mixer track, so no tap on
the mixer can separate them. The song is rendered once for each lane, and
once more for the mix when it is wanted or has to give an automatic tail
its length.

## Where it lives

- Engine: `crates/windfall-engine/src/stems/playlist.rs` —
  `playlist_stems`, `playlist_stem_project`, `render_playlist_stems`.
- Session: `session/playlist_export.rs` and `Session::start_export` in
  `session/export.rs`. Encoders, staging, progress and cancellation are the
  plain export's.
- IPC: `ExportPlaylistStems { tracks?, includeMix, numbered, folder }`,
  `ExportedFile.playlistTrack`, Tauri command `export_playlist_tracks`.
- UI: `features/export/playlist-stem-controls.tsx`. Turning it on turns
  mixer stems off and sets the render to the whole song.

Files are named like mixer stems: for `Song.wav`, `Song - Mix.wav` and
`Song - 01 Drums.wav`, in the folder `Song` or beside it. The number is the
lane's place on the timeline.

## Refused

Pattern mode; `stems` set as well; a lane the active arrangement does not
show; a lane that plays nothing; no lane and no mix.

## Not covered

No test measures the sidechain statement. A region export with playlist
tracks is allowed and untested. A lane that holds clips only outside the
region gives a silent file.

## Tests

`crates/windfall-engine/tests/engine/playlist_stems.rs` (run, 8 passed on
2026-10-10). `session/tests/playlist_export.rs` and
`features/export/playlist-tracks.qa.test.tsx` are written and not yet run.
