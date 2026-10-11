# Global editor snap

Windfall's desktop playlist and piano roll share `none`, `step`, `beat`, and
`bar`. `apps/desktop/src/lib/store/snap.ts` owns that session value in
`useSnapStore`, with `bar` as its default.

Both editors keep their existing `snap` read interface. The playlist's
`setSnap` writes to the shared owner. The piano roll's `setSnap` writes shared
choices to the same owner; its finer divisions, such as `step/2` or `beat/3`,
stay local to the piano roll and leave the playlist and shared value alone.
Subscriptions project each shared selection into both editor stores. A shared
selection always notifies, even when its value is unchanged, so selecting the
current shared choice again clears any piano-only division.

The stores outlive panel unmounts, so leaving an editor preserves the shared
choice and any piano-only division. `onProjectReplaced` resets the owner to
`bar` after New or Open and clears the piano-only division through those same
subscriptions. Snap is session-only, excluded from both editors' persisted UI
preferences and from the project document. Legacy persisted snap fields are
ignored; other UI preferences retain their existing persistence behavior.

Existing playlist and piano-roll snap math is unchanged. This seam only owns
the selected editor grid value. It does not quantize live MIDI. It does not
change Typing, sheet export, the riff generator, playlist playback, or the
engine, and it dispatches no project or engine commands.

Focused verification from `apps/desktop`:

`pnpm test src/lib/store/snap.test.ts`

The test covers selections in both directions for all four shared values,
piano-only divisions, reselecting an unchanged shared value, panel changes,
New and Open resetting to `bar`, and ignoring legacy snap preferences.

## Live input — 2026-10-10

The statement above that this seam does not quantize live MIDI is
superseded for the played-note log. Notes played on the computer keyboard
already entered that log through `auditionOn` and `auditionOff`, and "Dump
played notes" places them on the shared snap.

Notes from a MIDI input now enter the same log. The session emits a
`midi:note` event, `{ channel, key, velocity }` with velocity 0 for a
release, for every hardware note the engine accepts.
`connectStores` hands it to `logHardwareNote`, which logs the note and asks
for no audition, because the engine has sounded it already.

The snap itself is unchanged: still a session value, still reset to bar on
New and Open. Quantizing notes as they are recorded into a pattern belongs
to note recording, not to this seam.

Coverage, written and not yet run:
`features/piano-roll/live-input-snap.test.ts` and
`midi_hardware_notes_are_reported_to_the_ui_for_the_played_note_log`.
