# Windfall piano-roll riff generator

State: rewritten 2026-10-10 for the staged generator. The code is written
and has not been built or run. See
[the QA hand-off](../../qa/2026-10-10-piano.md).

`pianoRoll.generateRiff` / **Generate riff…** opens a dialog for the open
pattern channel. A riff is built in eight stages, each working on the notes
the one before left. Optional stages can be switched off, and a stage that
is off changes nothing.

1. **Progression** — key, scale, a named progression or typed degrees
   ("I V vi IV", "1-5-6-4", "ii7 V7 I"), triads, sevenths or ninths, bars
   per chord, length in bars, octave, and a seeded chance of swapping a
   chord. Writes one block chord per span.
2. **Chords** — inversion (or nearest voice leading), close, open or
   drop-two spread, an added bass note, strum offset.
3. **Arpeggio** — up, down, up-down, down-up, random or as played; rate
   from quarters to thirty-seconds with triplets; one to three octaves;
   gate; restart at each chord.
4. **Mirror** — pitch inversion around the chord or a chosen key, kept in
   the scale, and time reversal inside each chord.
5. **Levels** — base velocity, accents, seeded variation, a ramp across the
   phrase, pan, and release and modulation X/Y.
6. **Articulation** — length scaling, legato or staccato, tied repeats, a
   minimum length.
7. **Groove** — swing on the arpeggio's off-beats and seeded timing
   variation that never moves a note before the riff's first tick.
8. **Fit** — bring notes into a key range and the scale, drop exact
   duplicates.

One seed drives every random stage, each from a stream of its own, so
changing one stage's random settings does not reshuffle another's. The same
settings, seed, meter and draw velocity give the same notes.

The riff goes after the channel's last note, at the start of the pattern,
or in place of the selected notes. Apply is one command and one undo step:
`addNotes`, or a batch that also removes the replaced selection and grows
the pattern. A riff that would pass the longest pattern, or more than
1,024 notes, is refused with a message instead of being shortened. A
request is captured when the dialog opens and cannot apply after the
channel, the project revision or the document changed. **Audition** plays
the preview through the audition path and releases every note it started
when it stops or the dialog closes.

`riff/pipeline.test.ts` covers the pipeline as pure functions. The dialog,
audition and apply have no tests yet.
