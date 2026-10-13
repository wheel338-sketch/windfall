import { memo } from "react"

import { cn } from "@/lib/utils"

import { useDisplayCanvas } from "../use-display-canvas"
import { TRANSIENT_RANGE_DB, transientShape } from "./curves"
import { translatedText as t, message } from "@/lib/i18n"

const COLORS = {
  grid: "var(--wf-grid-line)",
  dry: "var(--muted-foreground)",
  shaped: "var(--wf-brand)",
}
const POINTS = 96

type TransientDisplayProps = {
  attack: number
  sustain: number
  /** Which part is let out: 0 both, 1 the attack, 2 the sustain. */
  listen: number
  className?: string
}

/**
 * One hit, as it comes in (the thin line) and as it goes out: taller or
 * flatter at the front with Attack, fuller or shorter behind it with
 * Sustain. The part that is not let out is left undrawn.
 */
export const TransientDisplay = memo(function TransientDisplay({
  attack,
  sustain,
  listen,
  className,
}: TransientDisplayProps) {
  const canvas = useDisplayCanvas(COLORS, (context, size, colors) => {
    const { width, height } = size
    const { dry, shaped } = transientShape(attack, sustain, POINTS)
    // Room for the tallest a hit can get: 18 dB over where it came in.
    const top = 10 ** (TRANSIENT_RANGE_DB / 20)
    const x = (index: number) => (index / (POINTS - 1)) * width
    const y = (level: number) =>
      height - (Math.sqrt(Math.min(level, top) / top) * (height - 4) + 2)
    const trace = (levels: number[], from: number, to: number) => {
      const path = new Path2D()
      path.moveTo(x(from), height)
      for (let index = from; index < to; index += 1) {
        path.lineTo(x(index), y(levels[index]))
      }
      path.lineTo(x(to - 1), height)
      return path
    }

    context.strokeStyle = colors.grid
    context.lineWidth = 1
    context.beginPath()
    context.moveTo(Math.round(width / 5) + 0.5, 0)
    context.lineTo(Math.round(width / 5) + 0.5, height)
    context.stroke()

    context.strokeStyle = colors.dry
    context.globalAlpha = 0.6
    context.stroke(trace(dry, 0, POINTS))
    context.globalAlpha = 1

    // The first fifth of the hit is what the detector calls attack.
    const split = Math.ceil(POINTS / 5)
    const [from, to] =
      listen === 1 ? [0, split] : listen === 2 ? [split, POINTS] : [0, POINTS]
    const shown = trace(shaped, from, to)
    context.fillStyle = colors.shaped
    context.globalAlpha = 0.16
    context.fill(shown)
    context.globalAlpha = 1
    context.strokeStyle = colors.shaped
    context.lineWidth = 1.75
    context.lineJoin = "round"
    context.stroke(shown)
  })

  const part =
    listen === 1 ? t("attack only") : listen === 2 ? t("sustain only") : t("whole hit")
  return (
    <div
      data-slot="transient-display"
      role="img"
      aria-label={message("A hit before and after shaping, {part}", { part })}
      className={cn(
        "relative h-20 overflow-hidden rounded-[5px] bg-(--wf-meter-bg) ring-1 ring-(--wf-grid-line-strong) in-data-enlarged:h-28",
        className
      )}
    >
      <canvas ref={canvas} className="absolute inset-0 size-full" />
    </div>
  )
})
