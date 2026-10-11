# Playlist track link label

Windfall's playlist track header reads the saved entry for its track ID from
`playlist.arrangementBook.linkedTracks`. An instrument link displays the channel's
name from `project.channels`; an audio link displays the sample's name from
`project.samples`. The name appears beside the existing track name. An unresolved
channel or sample displays `Missing link`. Tracks with no saved link, including
projects without an arrangement book, retain their existing header.

The header subscribes to the project store, so link changes, renamed channels or
samples, and removed references update the label without remounting the header.
The link display is a label only and dispatches no command. Drawing, playback,
and mixer routing are unchanged. The arrangement panel remains the link editor.

Focused verification from `apps/desktop`:

```powershell
pnpm test -- src/features/playlist/track-link.test.tsx
```

The suite covers unlinked and legacy projects, instrument and audio names, both
missing reference kinds, reactive store updates, and command-free label rendering
and clicks.

## Linked behaviour — 2026-10-10

The label is no longer the whole feature. It also names the mixer track of
an audio track. The header menu has a **Link track** submenu, pressing a
linked header selects its channel and mixer track without dispatching a
command, and the link changes playback and routing as
`arrangement.md` describes under "Linked tracks". The helpers are in
`features/playlist/linked-tracks.ts`.
