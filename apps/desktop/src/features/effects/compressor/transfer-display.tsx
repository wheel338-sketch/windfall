import { memo } from "react"

import type { CompressorParams } from "@/bindings"
import { cn } from "@/lib/utils"

import { useDisplayCanvas } from "../use-display-canvas"
import { outputLevelDb } from "./curve"
import { translatedText as t } from "@/lib/i18n"

/** The quietest level the display shows, in dB. The loudest is 0. */
export const DISPLAY_FLOOR_DB = -60
const GRID_STEP_DB = 12
const POINTS = 121

const COLORS = {
  grid: "var(--wf-grid-line)",
  axis: "var(--wf-grid-line-strong)",
  label: "var(--muted-foreground)",
  curve: "var(--wf-brand)",
  threshold: "var(--foreground)",
}

type TransferDisplayProps = {
  params: Pick<
    CompressorParams,
    "thresholdDb" | "ratio" | "kneeDb" | "makeupDb" | "autoMakeup" | "mix"
  >
  className?: string
}

/**
 * The compressor's curve: level in along the bottom, level out up the side.
 * The straight diagonal is a signal left alone; the curve leaves it at the
 * knee, which is shaded around the threshold line.
 */
export const TransferDisplay = memo(function TransferDisplay({
  params,
  className,
}: TransferDisplayProps) {
  const canvas = useDisplayCanvas(COLORS, (context, size, colors) => {
    const { width, height } = size
    const span = -DISPLAY_FLOOR_DB
    const x = (db: number) => ((db - DISPLAY_FLOOR_DB) / span) * width
    const y = (db: number) => height - ((db - DISPLAY_FLOOR_DB) / span) * height
    const crisp = (value: number) => Math.round(value) + 0.5

    context.lineWidth = 1
    context.strokeStyle = colors.grid
    context.beginPath()
    for (let db = DISPLAY_FLOOR_DB + GRID_STEP_DB; db < 0; db += GRID_STEP_DB) {
      context.moveTo(crisp(x(db)), 0)
      context.lineTo(crisp(x(db)), height)
      context.moveTo(0, crisp(y(db)))
      context.lineTo(width, crisp(y(db)))
    }
    context.stroke()

    // A signal that is left alone runs along this line.
    context.strokeStyle = colors.axis
    context.beginPath()
    context.moveTo(0, height)
    context.lineTo(width, 0)
    context.stroke()

    const kneeFrom = x(params.thresholdDb - params.kneeDb / 2)
    const kneeTo = x(params.thresholdDb + params.kneeDb / 2)
    context.fillStyle = colors.curve
    context.globalAlpha = 0.1
    context.fillRect(kneeFrom, 0, Math.max(0, kneeTo - kneeFrom), height)
    context.globalAlpha = 0.55
    context.strokeStyle = colors.threshold
    context.setLineDash([2, 3])
    context.beginPath()
    context.moveTo(crisp(x(params.thresholdDb)), 0)
    context.lineTo(crisp(x(params.thresholdDb)), height)
    context.stroke()
    context.setLineDash([])
    context.globalAlpha = 1

    const curve = new Path2D()
    for (let point = 0; point < POINTS; point += 1) {
      const level = DISPLAY_FLOOR_DB + (span * point) / (POINTS - 1)
      const out = outputLevelDb(params, level)
      if (point === 0) curve.moveTo(x(level), y(out))
      else curve.lineTo(x(level), y(out))
    }
    const area = new Path2D(curve)
    area.lineTo(width, height)
    area.lineTo(0, height)
    area.closePath()
    context.globalAlpha = 0.14
    context.fill(area)
    context.globalAlpha = 1
    context.strokeStyle = colors.curve
    context.lineWidth = 1.75
    context.lineJoin = "round"
    context.stroke(curve)

    context.fillStyle = colors.label
    context.font = "9px 'Martian Mono Variable', ui-monospace, monospace"
    context.textBaseline = "bottom"
    for (const db of [-48, -24]) {
      context.textAlign = "center"
      context.fillText(`−${-db}`, x(db), height - 2)
      context.textAlign = "left"
      context.fillText(`−${-db}`, 3, y(db) - 1)
    }
  })

  return (
    <div
      data-slot="transfer-display"
      role="img"
      aria-label={t("Compressor curve: level in against level out")}
      className={cn(
        "relative aspect-square overflow-hidden rounded-[5px] bg-(--wf-meter-bg) ring-1 ring-(--wf-grid-line-strong)",
        className
      )}
    >
      <canvas ref={canvas} className="absolute inset-0 size-full" />
    </div>
  )
})
