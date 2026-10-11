import { bySpan } from "./stages"
import type { RiffResult } from "./pipeline"

const WIDTH = 640
const HEIGHT = 144

/** The riff as a small piano roll: time left to right, pitch bottom to top. */
export function RiffPreview({ result }: { result: RiffResult | null }) {
  if (!result)
    return (
      <div
        role="img"
        aria-label="Riff preview: nothing to show"
        className="flex h-36 w-full items-center justify-center rounded-md border border-border bg-muted/40 text-muted-foreground"
      >
        Fix the settings below to see the riff.
      </div>
    )
  const { notes, start } = result
  const end = Math.max(
    result.end,
    ...notes.map((note) => note.start + note.length)
  )
  const low = Math.min(...notes.map((note) => note.key)) - 1
  const high = Math.max(...notes.map((note) => note.key)) + 1
  const perTick = WIDTH / Math.max(1, end - start)
  const row = HEIGHT / (high - low + 1)
  const bars: number[] = []
  for (let tick = start; tick < end; tick += result.context.barTicks)
    bars.push(tick)
  const chords = bySpan(notes).map((chord) => chord[0].span)
  return (
    <svg
      role="img"
      aria-label={`Riff preview: ${notes.length} notes`}
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      preserveAspectRatio="none"
      className="h-36 w-full rounded-md border border-border bg-muted/40"
    >
      {chords.map(
        (span) =>
          span.index % 2 === 1 && (
            <rect
              key={span.index}
              x={(span.start - start) * perTick}
              y={0}
              width={(span.end - span.start) * perTick}
              height={HEIGHT}
              className="fill-muted"
            />
          )
      )}
      {bars.map((tick) => (
        <line
          key={tick}
          x1={(tick - start) * perTick}
          x2={(tick - start) * perTick}
          y1={0}
          y2={HEIGHT}
          vectorEffect="non-scaling-stroke"
          className="stroke-border"
        />
      ))}
      {notes.map((note, index) => (
        <rect
          key={index}
          data-slot="riff-preview-note"
          data-start={note.start}
          data-key={note.key}
          data-length={note.length}
          x={(note.start - start) * perTick}
          y={(high - note.key) * row}
          width={Math.max(1.5, note.length * perTick - 0.5)}
          height={Math.max(1.5, row - 0.5)}
          opacity={0.35 + 0.65 * note.velocity}
          className={
            note.role === "bass" ? "fill-muted-foreground" : "fill-primary"
          }
        />
      ))}
    </svg>
  )
}
