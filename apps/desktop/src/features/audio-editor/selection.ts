export type Selection = { start: number; end: number }

export function frameAt(
  x: number,
  left: number,
  width: number,
  frames: number
) {
  if (width <= 0 || !Number.isFinite(x)) return 0
  return Math.round(Math.max(0, Math.min(1, (x - left) / width)) * frames)
}

export function selectionBetween(
  a: number,
  b: number,
  frames: number
): Selection {
  const start = Math.max(0, Math.min(frames - 1, Math.min(a, b)))
  return { start, end: Math.max(start + 1, Math.min(frames, Math.max(a, b))) }
}

export function validSelection(selection: Selection, frames: number) {
  return (
    Number.isInteger(selection.start) &&
    Number.isInteger(selection.end) &&
    selection.start >= 0 &&
    selection.start < selection.end &&
    selection.end <= frames
  )
}

/**
 * A clip position as minutes:seconds, such as `1:02.250`: what a range or
 * cue is read as, rather than a frame count.
 */
export function clockTime(seconds: number, decimals = 3): string {
  if (!Number.isFinite(seconds)) return ""
  const scale = 10 ** decimals
  // Rounded first, so 59.9996 s reads 1:00.000 and never 0:60.000.
  const total = Math.round(Math.max(0, seconds) * scale) / scale
  const minutes = Math.floor(total / 60)
  const rest = (total - minutes * 60).toFixed(decimals)
  return `${minutes}:${rest.padStart(decimals > 0 ? decimals + 3 : 2, "0")}`
}

const RULER_STEPS = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600]

/**
 * The marks of a time ruler over `duration` seconds: the finest round step
 * that gives at most `most` intervals, each mark with the decimals its step needs.
 */
export function rulerTicks(duration: number, most = 8): { seconds: number; decimals: number }[] {
  if (!Number.isFinite(duration) || duration <= 0) return []
  const step = RULER_STEPS.find((step) => duration / step <= most) ?? RULER_STEPS.at(-1)!
  const decimals = step < 0.01 ? 3 : step < 0.1 ? 2 : step < 1 ? 1 : 0
  const ticks = []
  for (let i = 0; i * step <= duration + 1e-9; i++)
    ticks.push({ seconds: Math.round(i * step * 1000) / 1000, decimals })
  return ticks
}
