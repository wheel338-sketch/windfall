/*
 * Where a finger is on the pads and which keys the controllers may show.
 * Everything here is a pure function, so it is tested without a layout.
 */

/** The softest a touch plays, as on the keys of `PianoKeyboard`. */
export const MIN_VELOCITY = 0.2
export const MAX_KEY = 127

export const PAD_COLUMNS = 4
export const PAD_ROWS = 4
export const PAD_COUNT = PAD_COLUMNS * PAD_ROWS
/** The first pad of the default bank: the kick of a General MIDI drum map. */
export const DEFAULT_PAD_BASE = 36
const MAX_PAD_BASE = MAX_KEY - PAD_COUNT + 1

export const OCTAVE_RANGES = [2, 3, 4] as const
export type OctaveRange = (typeof OCTAVE_RANGES)[number]
export const DEFAULT_LOW_KEY = 48
export const DEFAULT_OCTAVES: OctaveRange = 2

/** The part of a rectangle the hit tests read, as a `DOMRect` has it. */
export type SurfaceRect = {
  left: number
  top: number
  width: number
  height: number
}

/** Velocity from how far down a key or a pad was touched: lower is louder. */
export function velocityAt(share: number): number {
  const down = Number.isFinite(share) ? Math.min(1, Math.max(0, share)) : 0
  return MIN_VELOCITY + (1 - MIN_VELOCITY) * down
}

/**
 * The pad at a point of the grid, and the velocity for where in the pad the
 * point is. Pad 0 is at the bottom left and pad 15 at the top right, the
 * way drum pads are numbered. Null outside the grid.
 */
export function padAtPoint(
  x: number,
  y: number,
  rect: SurfaceRect
): { pad: number; velocity: number } | null {
  if (!(rect.width > 0) || !(rect.height > 0)) return null
  const across = ((x - rect.left) / rect.width) * PAD_COLUMNS
  const down = ((y - rect.top) / rect.height) * PAD_ROWS
  if (!(across >= 0 && across < PAD_COLUMNS && down >= 0 && down < PAD_ROWS))
    return null
  const column = Math.floor(across)
  const row = Math.floor(down)
  return {
    pad: (PAD_ROWS - 1 - row) * PAD_COLUMNS + column,
    velocity: velocityAt(down - row),
  }
}

export function isOctaveRange(value: unknown): value is OctaveRange {
  return OCTAVE_RANGES.some((range) => range === value)
}

/** The highest C a keyboard of this many octaves may start on. */
export function maxLowKey(octaves: OctaveRange): number {
  return 120 - 12 * (octaves - 1)
}

/** The nearest C the keyboard may start on. */
export function clampLowKey(lowKey: number, octaves: OctaveRange): number {
  const c = Number.isFinite(lowKey) ? Math.round(lowKey / 12) * 12 : 0
  return Math.min(maxLowKey(octaves), Math.max(0, c))
}

/**
 * The keys the keyboard shows: from a C up to the C `octaves` above it. The
 * highest position ends on key 127, a G, because MIDI ends there.
 */
export function keyboardSpan(
  lowKey: number,
  octaves: OctaveRange
): { low: number; high: number } {
  const low = clampLowKey(lowKey, octaves)
  return { low, high: Math.min(MAX_KEY, low + 12 * octaves) }
}

export function clampPadBase(base: number): number {
  const whole = Number.isFinite(base) ? Math.round(base) : DEFAULT_PAD_BASE
  return Math.min(MAX_PAD_BASE, Math.max(0, whole))
}

/**
 * The base of the bank above or below: 16 keys away, stopping at key 0 and
 * at the bank that ends on key 127. Coming back from either end lands on
 * the banks around the default again.
 */
export function stepPadBase(base: number, direction: -1 | 1): number {
  const from = clampPadBase(base)
  // The banks 16 apart that include the default one: 4, 20, 36 and so on.
  const first = DEFAULT_PAD_BASE % PAD_COUNT
  if (direction > 0) {
    const next = first + (Math.floor((from - first) / PAD_COUNT) + 1) * PAD_COUNT
    return next > MAX_PAD_BASE ? MAX_PAD_BASE : next
  }
  const previous =
    first + (Math.ceil((from - first) / PAD_COUNT) - 1) * PAD_COUNT
  return previous < 0 ? 0 : previous
}

/** How many banks of 16 pads a rack of this many channels fills. */
export function channelBankCount(channels: number): number {
  return Math.max(1, Math.ceil(channels / PAD_COUNT))
}
