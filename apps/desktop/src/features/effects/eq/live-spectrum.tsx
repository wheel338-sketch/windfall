import { useRef } from "react"

import {
  AnalyzerCanvas,
  type AnalyzerPainter,
} from "@/features/analyzers/analyzer-canvas"
import { dbPosition, spectrumCurve } from "@/features/analyzers/math"
import type { AnalyzerFrame } from "@/features/analyzers/types"

import { DISPLAY_MAX_HZ, DISPLAY_MIN_HZ, plotRect } from "./geometry"
import { translatedText as t } from "@/lib/i18n"

/** Points the spectrum is drawn at, along the display's frequency axis. */
export const SPECTRUM_POINTS = 160
/** The quietest level the spectrum shows, in dB. The loudest is 0. */
export const SPECTRUM_FLOOR_DB = -90
/** How fast a point falls back once the sound there has gone, per frame. */
const FALL_DB = 1.5

type LiveSpectrumProps = {
  /** The analyzer slot that carries the track's spectrum. */
  slot: string
}

/**
 * The spectrum of the audio on the equaliser's track, drawn behind the
 * curve on the same frequency axis. It is the analyzer's FFT of what the
 * track is putting out after its fader, so it shows the equaliser's effect
 * along with everything else on the track. It is not worked out from the
 * equaliser's settings, and with no audio it is not there.
 *
 * Its height is level, from -90 dB at the bottom of the plot to 0 dB at
 * the top, which is another scale than the curve's gain.
 */
export function EqLiveSpectrum({ slot }: LiveSpectrumProps) {
  const root = useRef<HTMLDivElement>(null)
  const shown = useRef<Float32Array | null>(null)

  const onFrame = (frame: AnalyzerFrame) => {
    const spectrum = frame.spectrum
    if (!spectrum || !frame.levels.some((level) => level.valid && !level.silent)) {
      onClear()
      return
    }
    const next = spectrumCurve(
      spectrum.left,
      spectrum.right,
      spectrum.binHz,
      SPECTRUM_POINTS,
      DISPLAY_MIN_HZ,
      DISPLAY_MAX_HZ
    )
    const held = (shown.current ??= new Float32Array(SPECTRUM_POINTS).fill(
      -Infinity
    ))
    let strongest = 0
    for (let point = 0; point < SPECTRUM_POINTS; point += 1) {
      held[point] = Math.max(next[point], held[point] - FALL_DB)
      if (next[point] > next[strongest]) strongest = point
    }
    const element = root.current
    if (element) {
      // The frequency at the middle of the strongest point, for a reader
      // that cannot see the picture.
      const ratio = DISPLAY_MAX_HZ / DISPLAY_MIN_HZ
      const hz = DISPLAY_MIN_HZ * ratio ** ((strongest + 0.5) / SPECTRUM_POINTS)
      if (Number.isFinite(next[strongest])) {
        element.dataset.peakHz = hz.toFixed(1)
        element.dataset.peakDb = next[strongest].toFixed(1)
      } else {
        delete element.dataset.peakHz
        delete element.dataset.peakDb
      }
    }
  }

  const onClear = () => {
    shown.current?.fill(-Infinity)
    const element = root.current
    if (element) {
      delete element.dataset.peakHz
      delete element.dataset.peakDb
    }
  }

  const paint: AnalyzerPainter = (context, size, colors, frame) => {
    const held = shown.current
    if (!frame || !held) return
    const plot = plotRect(size.width, size.height)
    const bottom = plot.top + plot.height
    const x = (point: number) =>
      plot.left + ((point + 0.5) / SPECTRUM_POINTS) * plot.width
    const y = (db: number) =>
      bottom - dbPosition(db, SPECTRUM_FLOOR_DB) * plot.height

    context.beginPath()
    context.moveTo(plot.left, bottom)
    for (let point = 0; point < SPECTRUM_POINTS; point += 1) {
      context.lineTo(x(point), y(held[point]))
    }
    context.lineTo(plot.left + plot.width, bottom)
    context.closePath()
    context.globalAlpha = 0.18
    context.fillStyle = colors.second
    context.fill()
    context.globalAlpha = 0.55
    context.strokeStyle = colors.second
    context.lineWidth = 1
    context.stroke()
    context.globalAlpha = 1
  }

  return (
    <div
      ref={root}
      data-slot="eq-live-spectrum"
      className="pointer-events-none absolute inset-0"
    >
      <AnalyzerCanvas
        slot={slot}
        label={t("Live spectrum of this track's output, 20 Hz to 20 kHz, minus 90 to 0 decibels")}
        paint={paint}
        onFrame={onFrame}
        onClear={onClear}
        className="absolute inset-0"
      />
    </div>
  )
}
