import { memo } from "react"

import { cn } from "@/lib/utils"

import { useDisplayCanvas } from "../use-display-canvas"
import {
  BANDS,
  DISPLAY_FLOOR_DB,
  DISPLAY_MAX_HZ,
  DISPLAY_MIN_HZ,
  effectiveCrossovers,
  frequencyPosition,
  levelPosition,
} from "./curves"

const COLORS = {
  grid: "var(--wf-grid-line)",
  edge: "var(--wf-grid-line-strong)",
  label: "var(--muted-foreground)",
  band: "var(--wf-brand)",
  threshold: "var(--foreground)",
}

export type BandSetting = {
  thresholdDb: number
  ratio: number
  /** The band limiter's ceiling, where the effect has one. */
  ceilingDb?: number
}

type BandDisplayProps = {
  lowCrossoverHz: number
  highCrossoverHz: number
  sampleRate: number
  bands: readonly [BandSetting, BandSetting, BandSetting]
  /** Says what the picture shows, for assistive technology. */
  label: string
  className?: string
}

/**
 * The three bands across the spectrum: where they are split, the level
 * each one starts turning its signal down at, and how hard. The shading
 * above a threshold is deeper for a higher ratio.
 */
export const BandDisplay = memo(function BandDisplay({
  lowCrossoverHz,
  highCrossoverHz,
  sampleRate,
  bands,
  label,
  className,
}: BandDisplayProps) {
  const [low, high] = effectiveCrossovers(
    lowCrossoverHz,
    highCrossoverHz,
    sampleRate
  )
  const canvas = useDisplayCanvas(COLORS, (context, size, colors) => {
    const { width, height } = size
    const x = (hz: number) => frequencyPosition(hz) * width
    const y = (db: number) => height - levelPosition(db) * height
    const crisp = (value: number) => Math.round(value) + 0.5

    context.lineWidth = 1
    context.strokeStyle = colors.grid
    context.beginPath()
    for (const hz of [100, 1_000, 10_000]) {
      context.moveTo(crisp(x(hz)), 0)
      context.lineTo(crisp(x(hz)), height)
    }
    for (let db = DISPLAY_FLOOR_DB + 12; db < 0; db += 12) {
      context.moveTo(0, crisp(y(db)))
      context.lineTo(width, crisp(y(db)))
    }
    context.stroke()

    const edges = [DISPLAY_MIN_HZ, low, high, DISPLAY_MAX_HZ]
    bands.forEach((band, index) => {
      const from = x(edges[index])
      const to = x(edges[index + 1])
      const top = y(band.thresholdDb)
      const slope = band.ratio >= 100 ? 1 : 1 - 1 / Math.max(1, band.ratio)
      context.fillStyle = colors.band
      context.globalAlpha = 0.08 + 0.4 * slope
      context.fillRect(from, 0, Math.max(0, to - from), top)
      context.globalAlpha = 1
      context.strokeStyle = colors.threshold
      context.beginPath()
      context.moveTo(from, crisp(top))
      context.lineTo(to, crisp(top))
      context.stroke()
      if (band.ceilingDb !== undefined) {
        context.setLineDash([2, 3])
        context.beginPath()
        context.moveTo(from, crisp(y(band.ceilingDb)))
        context.lineTo(to, crisp(y(band.ceilingDb)))
        context.stroke()
        context.setLineDash([])
      }
    })

    context.strokeStyle = colors.edge
    context.beginPath()
    for (const hz of [low, high]) {
      context.moveTo(crisp(x(hz)), 0)
      context.lineTo(crisp(x(hz)), height)
    }
    context.stroke()

    context.fillStyle = colors.label
    context.font = "9px 'Martian Mono Variable', ui-monospace, monospace"
    context.textBaseline = "bottom"
    context.textAlign = "center"
    BANDS.forEach((band, index) => {
      context.fillText(
        band.title,
        (x(edges[index]) + x(edges[index + 1])) / 2,
        height - 2
      )
    })
  })

  return (
    <div
      data-slot="band-display"
      role="img"
      aria-label={label}
      className={cn(
        "relative h-20 overflow-hidden rounded-[5px] bg-(--wf-meter-bg) ring-1 ring-(--wf-grid-line-strong) in-data-enlarged:h-28",
        className
      )}
    >
      <canvas ref={canvas} className="absolute inset-0 size-full" />
    </div>
  )
})
