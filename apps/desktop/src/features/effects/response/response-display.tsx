import type { ReactNode } from "react"

import { cn } from "@/lib/utils"

import {
  frequencyLines,
  frequencyToX,
  gainLines,
  gainToY,
  plotRect,
  xToFrequency,
} from "../eq/geometry"
import { useSampleRate } from "../sample-rate"
import { useDisplayCanvas } from "../use-display-canvas"
import { responseGrid, type ResponseGrid } from "./curve"

/** A curve in dB at each frequency of the grid it is given. */
export type ResponseCurve = (
  grid: ResponseGrid,
  sampleRate: number
) => Float32Array

/** Points a curve is worked out at, whatever the display's width. */
const POINTS = 192

const COLORS = {
  grid: "var(--wf-grid-line)",
  gridStrong: "var(--wf-grid-line-strong)",
  label: "var(--muted-foreground)",
  curve: "var(--foreground)",
  ghost: "var(--muted-foreground)",
} as const

type ResponseDisplayProps = {
  /** What the effect does to each frequency with the settings shown. */
  response: ResponseCurve
  /** Fainter curves behind it, such as the shapes a morph moves between. */
  ghosts?: readonly ResponseCurve[]
  /** The gain the display spans either side of 0 dB. */
  rangeDb?: number
  /** What a screen reader says the display shows. */
  label: string
  className?: string
  /** Laid over the plot. */
  children?: ReactNode
}

/**
 * The frequency response of a filter or an equalizer: 20 Hz to 20 kHz left
 * to right on a logarithmic axis, gain bottom to top. It only draws; the
 * controls that change the curve sit beside it.
 */
export function ResponseDisplay({
  response,
  ghosts,
  rangeDb = 18,
  label,
  className,
  children,
}: ResponseDisplayProps) {
  const sampleRate = useSampleRate()
  const canvas = useDisplayCanvas(COLORS, (context, size, colors) => {
    const plot = plotRect(size.width, size.height)
    const bottom = plot.top + plot.height
    context.lineWidth = 1
    context.font = "9px sans-serif"
    context.textBaseline = "top"
    context.textAlign = "center"
    for (const line of frequencyLines()) {
      const x = Math.round(frequencyToX(line.hz, plot)) + 0.5
      context.strokeStyle = line.decade ? colors.gridStrong : colors.grid
      context.beginPath()
      context.moveTo(x, plot.top)
      context.lineTo(x, bottom)
      context.stroke()
      if (line.label) {
        context.fillStyle = colors.label
        context.fillText(line.label, x, bottom + 4)
      }
    }
    for (const line of gainLines(rangeDb)) {
      const y = Math.round(gainToY(line.db, plot, rangeDb)) + 0.5
      context.strokeStyle = line.db === 0 ? colors.gridStrong : colors.grid
      context.beginPath()
      context.moveTo(plot.left, y)
      context.lineTo(plot.left + plot.width, y)
      context.stroke()
    }

    const xs = new Float32Array(POINTS)
    const frequencies = new Float64Array(POINTS)
    // A filter has no response at or above half the sample rate.
    const ceiling = 0.499 * sampleRate
    for (let index = 0; index < POINTS; index += 1) {
      xs[index] = plot.left + (index / (POINTS - 1)) * plot.width
      frequencies[index] = Math.min(ceiling, xToFrequency(xs[index], plot))
    }
    const grid = responseGrid(frequencies, sampleRate)
    const trace = (gains: Float32Array) => {
      context.beginPath()
      for (let index = 0; index < POINTS; index += 1) {
        // A steep cut reads far outside any canvas.
        const y = Math.min(
          2 * size.height,
          Math.max(-size.height, gainToY(gains[index], plot, rangeDb))
        )
        if (index === 0) context.moveTo(xs[index], y)
        else context.lineTo(xs[index], y)
      }
      context.stroke()
    }
    context.save()
    context.beginPath()
    context.rect(plot.left, plot.top, plot.width, plot.height)
    context.clip()
    context.lineWidth = 1
    context.globalAlpha = 0.45
    context.strokeStyle = colors.ghost
    for (const ghost of ghosts ?? []) trace(ghost(grid, sampleRate))
    context.globalAlpha = 1
    context.lineWidth = 1.5
    context.strokeStyle = colors.curve
    trace(response(grid, sampleRate))
    context.restore()
  })

  return (
    <div
      data-slot="response-display"
      role="img"
      aria-label={label}
      className={cn(
        "relative h-28 min-w-0 overflow-hidden rounded-md border border-border/80 bg-chassis/50",
        className
      )}
    >
      <canvas ref={canvas} aria-hidden className="absolute inset-0 size-full" />
      {children}
    </div>
  )
}
