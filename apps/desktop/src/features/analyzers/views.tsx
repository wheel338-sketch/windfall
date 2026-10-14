import { useRef } from "react"

import { AnalyzerCanvas, type AnalyzerPainter } from "./analyzer-canvas"
import { useAnalyzerFrames } from "./feed"
import {
  dbPosition,
  formatDbfs,
  History,
  powerToDb,
  spectrumCurve,
  triggerOffset,
} from "./math"
import type { AnalyzerFrame, AnalyzerLevel } from "./types"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

/*
 * The views of the signal scope. Each draws one slot's analyzer frames and
 * nothing else: with no frame it shows its empty state, and none of them
 * holds on to what an earlier source looked like.
 */

const MIN_HZ = 20
const MAX_HZ = 20_000
const CURVE_POINTS = 240
/** Rows the spectrogram keeps: about four seconds at thirty a second. */
export const SPECTROGRAM_ROWS = 128
const SPECTROGRAM_BANDS = 64

type ViewProps = { slot: string }

function emptyLabel(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  color: string,
  text: string
) {
  context.fillStyle = color
  context.font = "10px ui-sans-serif, system-ui, sans-serif"
  context.textAlign = "center"
  context.textBaseline = "middle"
  context.fillText(text, width / 2, height / 2)
}

/**
 * The newest window of audio as two traces, left over right. With the
 * trigger on it starts where the louder side crosses zero going up, so a
 * steady tone stands still. A sample that was not a number breaks the line.
 */
export function Oscilloscope({
  slot,
  span,
  triggered,
}: ViewProps & { span: number; triggered: boolean }) {
  const paint: AnalyzerPainter = (context, size, colors, frame) => {
    const { width, height } = size
    const lane = height / 2
    context.lineWidth = 1
    context.strokeStyle = colors.grid
    context.beginPath()
    for (const center of [lane / 2, lane * 1.5]) {
      context.moveTo(0, Math.round(center) + 0.5)
      context.lineTo(width, Math.round(center) + 0.5)
    }
    context.stroke()
    if (!frame || !frame.scope || frame.scope.left.length === 0) {
      emptyLabel(context, width, height, colors.label, t("No signal to draw"))
      return
    }
    const scope = frame.scope
    const shown = Math.min(span, scope.left.length)
    const louder =
      (frame.levels[1].peak ?? 0) > (frame.levels[0].peak ?? 0)
        ? scope.right
        : scope.left
    const start = triggered ? triggerOffset(louder, shown) : 0
    const sides = [scope.left, scope.right]
    for (let side = 0; side < 2; side += 1) {
      const samples = sides[side]
      const center = lane * (side + 0.5)
      const reach = lane / 2 - 1
      context.strokeStyle = side === 0 ? colors.trace : colors.second
      context.beginPath()
      let drawing = false
      for (let index = 0; index < shown; index += 1) {
        const sample = samples[start + index]
        if (sample === null || sample === undefined) {
          drawing = false
          continue
        }
        const x = (index / Math.max(1, shown - 1)) * width
        const y = center - Math.max(-1, Math.min(1, sample)) * reach
        if (drawing) context.lineTo(x, y)
        else context.moveTo(x, y)
        drawing = true
      }
      context.stroke()
    }
  }
  return (
    <AnalyzerCanvas
      slot={slot}
      label={t("Oscilloscope: the left channel above the right")}
      paint={paint}
    />
  )
}

/**
 * The spectrum of the newest window on a logarithmic frequency axis from
 * 20 Hz to 20 kHz: the FFT's own power per bin, the two sides averaged.
 */
export function SpectrumCurve({ slot, floorDb }: ViewProps & { floorDb: number }) {
  const paint: AnalyzerPainter = (context, size, colors, frame) => {
    const { width, height } = size
    context.lineWidth = 1
    context.strokeStyle = colors.grid
    context.beginPath()
    for (const hz of [100, 1_000, 10_000]) {
      const x =
        Math.round((Math.log(hz / MIN_HZ) / Math.log(MAX_HZ / MIN_HZ)) * width) +
        0.5
      context.moveTo(x, 0)
      context.lineTo(x, height)
    }
    for (let db = -30; db > floorDb; db -= 30) {
      const y = Math.round((1 - dbPosition(db, floorDb)) * height) + 0.5
      context.moveTo(0, y)
      context.lineTo(width, y)
    }
    context.stroke()
    const spectrum = frame?.spectrum
    if (!spectrum || (spectrum.left.length === 0 && spectrum.right.length === 0)) {
      emptyLabel(context, width, height, colors.label, t("No spectrum available"))
      return
    }
    const curve = spectrumCurve(
      spectrum.left,
      spectrum.right,
      spectrum.binHz,
      CURVE_POINTS,
      MIN_HZ,
      MAX_HZ
    )
    context.beginPath()
    context.moveTo(0, height)
    for (let point = 0; point < CURVE_POINTS; point += 1) {
      const x = ((point + 0.5) / CURVE_POINTS) * width
      context.lineTo(x, (1 - dbPosition(curve[point], floorDb)) * height)
    }
    context.lineTo(width, height)
    context.closePath()
    context.globalAlpha = 0.25
    context.fillStyle = colors.trace
    context.fill()
    context.globalAlpha = 1
    context.strokeStyle = colors.trace
    context.stroke()
  }
  return (
    <AnalyzerCanvas
      slot={slot}
      label={message("Spectrum, 20 Hz to 20 kHz, {floorDb} to 0 decibels", { floorDb: plainNumber(floorDb) })}
      paint={paint}
    />
  )
}

/**
 * The spectrum over time, newest at the bottom: one row for every analysis
 * window the service sent, in 64 bands from DC to half the sample rate. A
 * break in the audio leaves an empty row and is never bridged.
 */
export function Spectrogram({ slot, floorDb }: ViewProps & { floorDb: number }) {
  const rows = useRef<History | null>(null)
  const history = () =>
    (rows.current ??= new History(SPECTROGRAM_ROWS, SPECTROGRAM_BANDS))

  const onFrame = (frame: AnalyzerFrame) => {
    const kept = history()
    if (frame.gap && kept.length > 0) kept.push([])
    for (const row of frame.history) kept.push(row.bands)
    if (frame.spectrum && frame.spectrum.bands.length > 0) {
      kept.push(frame.spectrum.bands)
    }
  }
  const onClear = () => rows.current?.clear()

  const paint: AnalyzerPainter = (context, size, colors) => {
    const { width, height } = size
    const kept = history()
    if (kept.length === 0) {
      emptyLabel(context, width, height, colors.label, t("No spectrogram available"))
      return
    }
    const rowHeight = height / SPECTROGRAM_ROWS
    const bandWidth = width / SPECTROGRAM_BANDS
    context.fillStyle = colors.trace
    for (let row = 0; row < kept.length; row += 1) {
      const y = height - (kept.length - row) * rowHeight
      for (let band = 0; band < SPECTROGRAM_BANDS; band += 1) {
        const power = kept.at(row, band)
        if (Number.isNaN(power)) continue
        const strength = dbPosition(powerToDb(power), floorDb)
        if (strength <= 0) continue
        context.globalAlpha = strength
        context.fillRect(
          band * bandWidth,
          y,
          Math.ceil(bandWidth),
          Math.ceil(rowHeight)
        )
      }
    }
    context.globalAlpha = 1
  }
  return (
    <AnalyzerCanvas
      slot={slot}
      label={t("Spectrogram: low to high frequency left to right, newest at the bottom")}
      paint={paint}
      onFrame={onFrame}
      onClear={onClear}
    />
  )
}

/**
 * The stereo picture of the newest window: each point is one sample, up
 * for what the two sides share and sideways for how they differ. A mono
 * signal is a vertical line, an out-of-phase one a horizontal line.
 */
export function Vectorscope({ slot }: ViewProps) {
  const paint: AnalyzerPainter = (context, size, colors, frame) => {
    const { width, height } = size
    const reach = Math.min(width, height) / 2 - 2
    const [centerX, centerY] = [width / 2, height / 2]
    context.lineWidth = 1
    context.strokeStyle = colors.grid
    context.beginPath()
    context.moveTo(centerX + 0.5, centerY - reach)
    context.lineTo(centerX + 0.5, centerY + reach)
    context.moveTo(centerX - reach, centerY + 0.5)
    context.lineTo(centerX + reach, centerY + 0.5)
    context.stroke()
    const points = frame?.vector
    if (!points || points.length === 0) {
      emptyLabel(context, width, height, colors.label, t("No stereo picture"))
      return
    }
    context.fillStyle = colors.trace
    for (const [mid, side] of points) {
      // Left leans left: the side signal is left minus right.
      const x = centerX - Math.max(-1, Math.min(1, side)) * reach
      const y = centerY - Math.max(-1, Math.min(1, mid)) * reach
      context.fillRect(x - 1, y - 1, 2, 2)
    }
  }
  return (
    <AnalyzerCanvas
      slot={slot}
      label={t("Vectorscope: mid upward, side across")}
      paint={paint}
    />
  )
}

const HOLD_MS = 1500
const SIDES = ["Left", "Right"] as const

type Held = { db: number; at: number }

function describeLevel(level: AnalyzerLevel | undefined): string {
  if (!level) return t("No level")
  if (!level.valid) return t("Not a valid signal")
  if (level.silent) return t("Silent")
  return formatDbfs(level.peakDb)
}

/**
 * A large stereo peak meter: a bar and a figure per side, the RMS level
 * beside the peak, a line that holds the highest peak for a moment, and a
 * clip light that stays on until it is clicked. A side whose window had a
 * sample that was not a number says so and shows no level.
 */
export function PeakMeter({ slot, floorDb }: ViewProps & { floorDb: number }) {
  const root = useRef<HTMLDivElement>(null)
  const held = useRef<Held[]>([
    { db: -Infinity, at: 0 },
    { db: -Infinity, at: 0 },
  ])
  const clipped = useRef(false)

  useAnalyzerFrames(slot, {
    onFrame(frame) {
      if (frame.levels.some((level) => level.clipped > 0)) clipped.current = true
    },
    onClear() {
      held.current = [
        { db: -Infinity, at: 0 },
        { db: -Infinity, at: 0 },
      ]
      clipped.current = false
    },
    paint(frame) {
      const element = root.current
      if (!element) return
      const now = performance.now()
      element.toggleAttribute("data-live", frame !== null)
      element.toggleAttribute("data-clipped", clipped.current)
      SIDES.forEach((name, side) => {
        const level = frame?.levels[side]
        const row = element.querySelector<HTMLElement>(
          `[data-side="${name.toLowerCase()}"]`
        )
        if (!row) return
        const peakDb =
          level?.valid && !level.silent ? (level.peakDb ?? -Infinity) : -Infinity
        const rmsDb =
          level?.valid && !level.silent ? (level.rmsDb ?? -Infinity) : -Infinity
        const hold = held.current[side]
        if (peakDb >= hold.db || now - hold.at > HOLD_MS) {
          held.current[side] = { db: peakDb, at: now }
        }
        const bar = row.querySelector<HTMLElement>("[data-part=peak]")
        const average = row.querySelector<HTMLElement>("[data-part=rms]")
        const line = row.querySelector<HTMLElement>("[data-part=hold]")
        const text = row.querySelector<HTMLElement>("[data-part=readout]")
        const meter = row.querySelector<HTMLElement>("[role=meter]")
        if (bar) {
          bar.style.transform = `scaleX(${dbPosition(peakDb, floorDb).toFixed(4)})`
        }
        if (average) {
          average.style.transform = `scaleX(${dbPosition(rmsDb, floorDb).toFixed(4)})`
        }
        if (line) {
          const at = dbPosition(held.current[side].db, floorDb)
          line.style.left = `${(at * 100).toFixed(2)}%`
          line.hidden = at <= 0
        }
        const spoken = describeLevel(level)
        if (text) {
          text.textContent =
            level?.valid && !level.silent && level.rmsDb !== null
              ? `${spoken} · RMS ${formatDbfs(level.rmsDb)}`
              : spoken
        }
        if (meter) {
          meter.setAttribute("aria-valuetext", spoken)
          if (Number.isFinite(peakDb)) {
            meter.setAttribute("aria-valuenow", peakDb.toFixed(1))
          } else {
            meter.removeAttribute("aria-valuenow")
          }
        }
        row.dataset.peakDb = Number.isFinite(peakDb) ? peakDb.toFixed(2) : ""
        row.toggleAttribute("data-invalid", level !== undefined && !level.valid)
      })
    },
  })

  return (
    <div
      ref={root}
      role="group"
      aria-label={t("Peak level")}
      data-slot="scope-meter"
      className="group/meter flex h-full flex-col justify-center gap-3 rounded-md bg-display p-4"
    >
      {SIDES.map((name) => (
        <div
          key={name}
          data-side={name.toLowerCase()}
          className="flex flex-col gap-1"
        >
          <div className="flex items-baseline justify-between font-readout tabular-nums">
            <span className="text-xs text-display-dim">{t(name)}</span>
            <span
              data-part="readout"
              className="text-base text-display-foreground"
            >
              {t("No level")}
            </span>
          </div>
          <div
            role="meter"
            aria-label={message("{name} peak level", { name })}
            aria-valuemin={floorDb}
            aria-valuemax={0}
            aria-valuetext={t("No level")}
            className="relative h-4 overflow-hidden rounded-sm bg-(--wf-meter-bg)"
          >
            <span
              data-part="peak"
              className="absolute inset-0 origin-left bg-(--wf-meter-low)"
              style={{ transform: "scaleX(0)" }}
            />
            <span
              data-part="rms"
              className="absolute inset-x-0 bottom-0 h-1 origin-left bg-(--wf-meter-mid)"
              style={{ transform: "scaleX(0)" }}
            />
            <span
              data-part="hold"
              hidden
              className="absolute inset-y-0 w-px bg-foreground"
            />
          </div>
        </div>
      ))}
      <button
        type="button"
        data-slot="scope-clip"
        aria-label={t("Clip light. Click to clear it")}
        onClick={() => {
          clipped.current = false
          root.current?.removeAttribute("data-clipped")
        }}
        className="self-end rounded-sm px-2 py-0.5 font-readout text-[10px] text-display-dim ring-1 ring-(--wf-grid-line-strong) outline-none group-data-clipped/meter:bg-(--wf-meter-high) group-data-clipped/meter:text-background focus-visible:ring-2 focus-visible:ring-ring"
      >
        {t("Clip")}
      </button>
    </div>
  )
}
