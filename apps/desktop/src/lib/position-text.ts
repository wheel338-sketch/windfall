import type { MeterChange, TimeSignature } from "@/bindings"

import { ticksPerBar, ticksPerBeat } from "./time"
import { meterSegments, musicalPosition } from "./timeline"
import { MAX_SONG_TICKS } from "./units"

// Bar, then optional beat and tick, split by ":" or ".": "9", "9:2", "9.2.480".
const POSITION = /^\s*(\d+)(?:\s*[:.]\s*(\d+)(?:\s*[:.]\s*(\d+))?)?\s*$/

/**
 * A song tick as a position to read and type: `bar:beat`, with `:tick`
 * added when it is not on a beat. Bars and beats count from 1 and follow
 * the song's meter changes; the tick counts into the beat.
 */
export function formatPositionText(
  tick: number,
  legacy: TimeSignature,
  meters: readonly MeterChange[] = []
): string {
  const position = musicalPosition(tick, legacy, meters)
  const base = `${position.bar}:${position.beat}`
  return position.tick === 0 ? base : `${base}:${position.tick}`
}

/**
 * The song tick for `bar`, `bar:beat` or `bar:beat:tick`. Null when the
 * text is not a position in the song: a beat past the bar, a tick past the
 * beat, a bar cut short by a meter change, or past the song's end.
 */
export function parsePositionText(
  text: string,
  legacy: TimeSignature,
  meters: readonly MeterChange[] = []
): number | null {
  const match = POSITION.exec(text)
  if (!match) return null
  const bar = Number(match[1])
  const beat = Number(match[2] ?? 1)
  const offset = Number(match[3] ?? 0)
  if (bar < 1 || beat < 1) return null
  const segment = meterSegments(legacy, meters).findLast((s) => s.bar <= bar)
  if (!segment) return null
  const beatTicks = ticksPerBeat(segment.signature)
  if (beat > segment.signature.numerator || offset >= beatTicks) return null
  const tick =
    segment.start +
    (bar - segment.bar) * ticksPerBar(segment.signature) +
    (beat - 1) * beatTicks +
    offset
  if (tick > MAX_SONG_TICKS) return null
  // The last bar before an unaligned meter change is shorter than its meter.
  if (segment.end < MAX_SONG_TICKS && tick >= segment.end) return null
  return tick
}
