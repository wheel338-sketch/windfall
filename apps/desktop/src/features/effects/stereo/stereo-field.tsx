import { cn } from "@/lib/utils"

import { fieldExtent } from "./stereo"

type StereoFieldProps = {
  /** 0 is mono, 1 is the image as it came in, 2 is twice as wide. */
  width: number
  /** -1 favors the left side, 1 the right. */
  balance?: number
  /** What a screen reader says the strip shows. */
  label: string
  className?: string
}

/**
 * How wide a stereo effect makes the image: a bar that grows out from the
 * center between L and R. It is a picture of the settings, not a meter.
 */
export function StereoField({
  width,
  balance = 0,
  label,
  className,
}: StereoFieldProps) {
  const { left, right } = fieldExtent(width, balance)
  return (
    <div
      data-slot="stereo-field"
      role="img"
      aria-label={label}
      className={cn(
        "relative h-6 min-w-0 overflow-hidden rounded-md border border-border/80 bg-chassis/50 text-[9px] leading-none text-muted-foreground",
        className
      )}
    >
      <div
        data-slot="stereo-field-image"
        className="absolute inset-y-1.5 rounded-sm bg-(--wf-control-fill)"
        style={{ left: `${left}%`, width: `${Math.max(0.5, right - left)}%` }}
      />
      <div className="absolute inset-y-0 left-1/2 w-px bg-(--wf-grid-line-strong)" />
      <span className="absolute top-1/2 left-1 -translate-y-1/2">L</span>
      <span className="absolute top-1/2 right-1 -translate-y-1/2">R</span>
    </div>
  )
}
