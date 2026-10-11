/*
 * The arithmetic the analyzer views share. Nothing here measures anything:
 * every number comes from a native frame, and these only place it on a
 * display.
 */

/** A mean-square power as dB. Silence is minus infinity. */
export function powerToDb(power: number): number {
  return power > 0 ? 10 * Math.log10(power) : -Infinity
}

/** A linear level as dB relative to full scale. Silence is minus infinity. */
export function levelToDb(level: number): number {
  return level > 0 ? 20 * Math.log10(level) : -Infinity
}

/** Where `db` sits between `floorDb` (0) and `topDb` (1), held to that range. */
export function dbPosition(db: number, floorDb: number, topDb = 0): number {
  if (!Number.isFinite(db)) return db > 0 ? 1 : 0
  return Math.min(1, Math.max(0, (db - floorDb) / (topDb - floorDb)))
}

/** "−12.3 dBFS", or "−∞ dBFS" for silence and "–" for no measurement. */
export function formatDbfs(db: number | null | undefined): string {
  if (db === null || db === undefined || Number.isNaN(db)) return "–"
  if (db === -Infinity) return "−∞ dBFS"
  const rounded = Math.round(db * 10) / 10
  const sign = rounded < 0 ? "−" : rounded > 0 ? "+" : ""
  return `${sign}${Math.abs(rounded).toFixed(1)} dBFS`
}

/**
 * Where an oscilloscope starts drawing so a steady wave stands still: the
 * first place the signal crosses zero going up, among the samples that
 * leave `span` more to draw. Zero when it never does.
 */
export function triggerOffset(
  samples: ArrayLike<number | null>,
  span: number
): number {
  const last = samples.length - span
  for (let index = 1; index <= last; index += 1) {
    const before = samples[index - 1]
    const here = samples[index]
    if (before !== null && here !== null && before < 0 && here >= 0) {
      return index
    }
  }
  return 0
}

/**
 * A spectrum as `points` values in dB along a logarithmic frequency axis
 * from `minHz` to `maxHz`. Each point takes the strongest bin its stretch
 * of the axis covers, or the bin it falls in where bins are wider than
 * points, and the sides that have a spectrum are averaged. A stretch above
 * half the sample rate, and a spectrum with neither side, give minus
 * infinity.
 */
export function spectrumCurve(
  left: ArrayLike<number>,
  right: ArrayLike<number>,
  binHz: number,
  points: number,
  minHz: number,
  maxHz: number
): Float32Array {
  const curve = new Float32Array(points).fill(-Infinity)
  const sides = [left, right].filter((side) => side.length > 0)
  if (sides.length === 0 || !(binHz > 0) || points < 1) return curve
  const bins = sides[0].length
  const ratio = maxHz / minHz
  for (let point = 0; point < points; point += 1) {
    const from = minHz * ratio ** (point / points)
    const to = minHz * ratio ** ((point + 1) / points)
    const first = Math.round(from / binHz)
    const end = Math.max(first + 1, Math.round(to / binHz))
    if (first >= bins) break
    let strongest = 0
    for (let bin = first; bin < Math.min(end, bins); bin += 1) {
      let power = 0
      for (const side of sides) power += side[bin] ?? 0
      strongest = Math.max(strongest, power / sides.length)
    }
    curve[point] = powerToDb(strongest)
  }
  return curve
}

const NOTE_NAMES = [
  "C",
  "C♯",
  "D",
  "D♯",
  "E",
  "F",
  "F♯",
  "G",
  "G♯",
  "A",
  "A♯",
  "B",
]

/** "A4" for MIDI note 69. Middle C, note 60, is C4. */
export function noteName(midi: number): string {
  const note = Math.round(midi)
  return `${NOTE_NAMES[((note % 12) + 12) % 12]}${Math.floor(note / 12) - 1}`
}

/** "+31.8 ¢", "−4.0 ¢", "0.0 ¢". */
export function formatCents(cents: number): string {
  const rounded = Math.round(cents * 10) / 10
  const sign = rounded < 0 ? "−" : rounded > 0 ? "+" : ""
  return `${sign}${Math.abs(rounded).toFixed(1)} ¢`
}

/**
 * A fixed-length history that forgets its oldest entry when full. `width`
 * values per entry, oldest first when read.
 */
export class History {
  readonly width: number
  readonly capacity: number
  private readonly values: Float32Array
  private head = 0
  private count = 0

  constructor(capacity: number, width: number) {
    this.capacity = capacity
    this.width = width
    this.values = new Float32Array(capacity * width)
  }

  get length(): number {
    return this.count
  }

  clear() {
    this.head = 0
    this.count = 0
  }

  /** Adds an entry. Missing values are stored as NaN. */
  push(entry: ArrayLike<number>) {
    const at = this.head * this.width
    for (let index = 0; index < this.width; index += 1) {
      this.values[at + index] = index < entry.length ? entry[index] : NaN
    }
    this.head = (this.head + 1) % this.capacity
    this.count = Math.min(this.count + 1, this.capacity)
  }

  /** Value `index` of entry `entry`, where entry 0 is the oldest kept. */
  at(entry: number, index: number): number {
    const slot =
      (this.head + this.capacity - this.count + entry) % this.capacity
    return this.values[slot * this.width + index]
  }
}
