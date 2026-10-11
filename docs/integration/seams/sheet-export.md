# Piano-roll sheet export

State: rewritten 2026-10-10. Everything described here is written and has
not been built, run or looked at on a page. See
[the QA hand-off](../../qa/2026-10-10-piano.md).

**Export sheet music…** (`pianoRoll.exportSheetMusic`) opens a dialog for the
open pattern channel. It engraves the channel's notes as staff notation,
shows the pages, and saves a PDF, prints, or saves MusicXML. It captures
the pattern when it opens and edits nothing.

## What is engraved

`score/model.ts` turns the lane into measures:

- Meter from the pattern's own signature and meter changes, else the
  project's, else 4/4; a change of meter starts a measure with a new time
  signature.
- Starts and ends snap to a grid of eighths, sixteenths or thirty-seconds
  (the dialog's choice), with eighth and sixteenth triplets recognised.
  Notes that had to move are counted and the dialog says how many.
- Same-start, same-length notes are a chord. At most two voices per staff;
  further overlaps are merged or shortened and counted.
- Durations are whole to thirty-second notes with one or two dots, tied
  across beats and barlines; rests fill the gaps and an empty bar is a
  whole-measure rest.
- Grand staff split at middle C, or a single treble or bass staff.
- Key signature chosen or detected from the notes; spelling follows the
  key; an accidental holds for its bar and line, and naturals cancel.
- Eighths and shorter are beamed within a beat.

`score/layout.ts` breaks measures into justified systems and pages on A4 or
US Letter, with title, channel name, author, tempo marking, measure numbers
and page numbers. Glyphs are Windfall's own vector shapes in
`score/glyphs.ts`; no music font is used.

## Output

A page is a list of drawing ops in points. `score/svg.ts` renders it for
the preview and for printing; `score/pdf.ts` writes a PDF 1.4 file with
vector pages and the standard Times and Helvetica fonts by name, nothing
embedded, in plain ASCII. **Print…** puts one SVG per sheet in a print-only
container, calls the system print dialog and removes the container again.

`Backend.sheetMusicSave(suggestedName, text, format?)` takes `"musicxml"`
(the default) or `"pdf"` to choose the extension and file filter. The
native `write_sheet_music` command is unchanged: it writes the text as
UTF-8, which an ASCII PDF survives.

## Limits

No lyrics, dynamics, articulations, slurs, grace notes, ottava lines or
chord symbols. Two voices per staff. Tuplets other than eighth and
sixteenth triplets are quantised to the straight grid. Velocity, pan,
expression and slides are not shown. One channel per score.

## Tests

`score/render.test.ts` covers the SVG and PDF writers, pagination, page
bounds and key detection. The notation rules above have no tests yet.
`sheet-music.test.ts` still covers the MusicXML builder.
