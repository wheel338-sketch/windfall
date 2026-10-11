# Piano-roll chord readout seam

The piano-roll toolbar shows a persistent readout next to the **Chords** button,
in an element with `aria-label="Chord"`. It reuses the existing `detectChord`
without changing its algorithm or labels. Recognition remains triads only;
unmatched pitch sets display the detector's `Unknown chord (...)` label.

The pure `chordReadout` helper accepts only the open channel's notes. Selection
wins: if any of those notes are selected, their keys determine the label,
regardless of their timing. Otherwise, a numeric session playhead is floored,
and notes contribute exactly when `start <= floor(playhead) < start + length`.
Ended notes and notes on other channels are excluded. With no contributing
keys, the readout displays an em dash and does not call `detectChord`.

`ChordReadout` subscribes through `editor.subscribe` and `session.onPlayhead`,
using the label as a stable string snapshot. Both subscriptions are removed on
unmount. The readout does not insert notes, dispatch commands, or change the
Chord tools dialog.

From `apps/desktop`, run:

```powershell
pnpm test -- src/features/piano-roll/chords
```

The focused tests cover selection precedence, playhead updates and flooring,
exclusive note ends, channel isolation, empty and unknown results, and the
absence of dispatches. Existing chord tool tests also run in this suite.

## 2026-10-10: wider vocabulary, live keys and a panel

Written and not executed; see [the QA hand-off](../../qa/2026-10-10-piano.md).
The sections above describe the earlier triad-only readout.

`detectChord` now names a single note, a two-note interval (a perfect fifth
as a power chord), the six triads as before, and the sixths, sevenths,
extended and altered chords in `CHORDS`. Three or more pitch classes are
tried on every sounding root; a seventh or larger chord may omit its fifth.
Among several readings the first of these decides: the root is the bass;
fewest omitted and altered tones; the lowest root pitch class from C; the
order of `CHORDS`. So C-E-G-A is a C sixth over C, E or G and an A minor
seventh over A. A bass that is not the root is written as a slash chord.
Sets that match nothing keep the `Unknown chord (...)` label.

The readout now has three sources, taken in this order: keys held live,
then the selected notes, then the notes under the playhead. Held keys come
from `chords/held-keys.ts`, which the audition path feeds: the typing
keyboard, step entry, the key gutter and a note being drawn or dragged. A
key counts once and its first release lets it go. Keys played on a hardware
MIDI device are not included: nothing carries held hardware notes to the
frontend.

Clicking the readout, or **Chord display** (`pianoRoll.chordDisplay`) in
the View menu and palette, opens a panel with the symbol, the long name,
the bass, the pitch classes and which source is shown.
