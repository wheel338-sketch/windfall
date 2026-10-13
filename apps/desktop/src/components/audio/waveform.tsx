// SPDX-License-Identifier: MIT
import * as React from "react"

import { cn } from "@/lib/utils"

import {
  observeCanvas,
  resolveColors,
  subscribeTheme,
  type CanvasSize,
} from "./canvas"
import { formatDb, formatPercent, gainToDb } from "./units"
import { useDragValue } from "./use-drag-value"
import { message, translatedText as t } from "@/lib/i18n"

export type WaveformPeaks = Float32Array | readonly number[]

export type WaveformHandle = {
  /** Moves the playhead to a 0 to 1 position, or hides it with null. */
  setPlayhead: (position: number | null) => void
}

/**
 * Draws min and max pairs (min, max, min, max...) as one vertical line per
 * device pixel column.
 */
export function drawWaveform(
  context: CanvasRenderingContext2D,
  peaks: WaveformPeaks,
  width: number,
  height: number,
  color: string,
  amplitude = 1
): void {
  const buckets = Math.floor(peaks.length / 2)
  if (buckets === 0 || width <= 0) {
    return
  }
  const middle = height / 2
  const reach = (height / 2) * amplitude
  context.fillStyle = color
  for (let x = 0; x < width; x += 1) {
    const from = Math.floor((x / width) * buckets)
    const to = Math.max(from + 1, Math.floor(((x + 1) / width) * buckets))
    let low = Infinity
    let high = -Infinity
    for (let bucket = from; bucket < to && bucket < buckets; bucket += 1) {
      low = Math.min(low, peaks[bucket * 2])
      high = Math.max(high, peaks[bucket * 2 + 1])
    }
    if (low > high) {
      continue
    }
    const top = Math.max(0, Math.floor(middle - high * reach))
    const bottom = Math.min(height, Math.ceil(middle - low * reach))
    context.fillRect(x, top, 1, Math.max(1, bottom - top))
  }
}

/** The most a normalized waveform is magnified: 100 times, or 40 dB. */
export const MAX_DISPLAY_GAIN = 100

// A display gain closer to 1 than this many dB is not worth a label.
const GAIN_LABEL_DB = 2

/**
 * How much a normalized waveform is magnified so its loudest peak fills the
 * height. The gain stops at `MAX_DISPLAY_GAIN`, so silence stays flat and a
 * noise floor is not drawn as a loud sound.
 */
export function waveformDisplayGain(peaks: WaveformPeaks): number {
  let loudest = 0
  for (let index = 0; index < peaks.length; index += 1) {
    const size = Math.abs(peaks[index])
    if (size > loudest) {
      loudest = size
    }
  }
  return loudest > 0 ? Math.min(MAX_DISPLAY_GAIN, 1 / loudest) : 1
}

/** A display gain as the corner label prints it: "×2.5", "×12", "×0.5". */
export function formatDisplayGain(gain: number): string {
  return `×${gain >= 9.95 ? gain.toFixed(0) : Number(gain.toFixed(1))}`
}

// Sizes of a region handle's parts in pixels.
const GRAB_WIDTH = 12
const FLAG_WIDTH = 8

export type RegionHandleLayout = {
  /** Left edge of the area that takes the pointer. */
  grab: number
  /** Left edge of the one-pixel line that marks the position. */
  line: number
  /** Left edge of the flag at the end of the line. */
  flag: number
}

/**
 * Where the parts of a region handle sit, in pixels from the left edge of a
 * waveform `width` pixels wide. Away from the edges the grab area is centered
 * on the position. Near an edge everything slides inwards and stays whole, so
 * a handle resting at 0 or 1 can be grabbed across its full width.
 */
export function layoutRegionHandle(
  edge: "start" | "end",
  position: number,
  width: number
): RegionHandleLayout {
  const inside = (left: number, size: number) =>
    Math.max(0, Math.min(width - size, left))
  const at = position * width
  const line = inside(at - 0.5, 1)
  return {
    grab: inside(at - GRAB_WIDTH / 2, GRAB_WIDTH),
    line,
    flag: inside(edge === "start" ? line : line + 1 - FLAG_WIDTH, FLAG_WIDTH),
  }
}

type RegionHandleProps = {
  edge: "start" | "end"
  /** Width of the waveform in pixels. */
  width: number
  value: number
  min: number
  max: number
  label: string
  disabled: boolean
  format: (position: number) => string
  onValueChange?: (position: number) => void
  onGestureStart?: () => void
  onGestureEnd?: () => void
}

function RegionHandle({
  edge,
  width,
  value,
  min,
  max,
  label,
  disabled,
  format,
  onValueChange,
  onGestureStart,
  onGestureEnd,
}: RegionHandleProps) {
  const control = useDragValue({
    value,
    min,
    max,
    onValueChange,
    onGestureStart,
    onGestureEnd,
    defaultValue: edge === "start" ? 0 : 1,
    format,
    disabled,
    orientation: "horizontal",
    trackOvershoot: true,
    wheel: false,
    editable: false,
    // The handle follows the pointer: its full range covers this share of
    // the waveform's width.
    dragRange: (element) =>
      (element.parentElement?.clientWidth ?? 0) * (max - min),
  })
  const place = layoutRegionHandle(edge, control.value, width)

  return (
    <div
      {...control.sliderProps}
      data-slot="waveform-handle"
      data-edge={edge}
      aria-label={label}
      className="group/handle absolute inset-y-0 z-10 cursor-ew-resize touch-none outline-none data-disabled:cursor-default"
      style={{ left: place.grab, width: GRAB_WIDTH }}
    >
      <div
        className={cn(
          // With focus the line thickens towards the inside of the region.
          "absolute inset-y-0 w-px bg-(--wf-brand) group-focus-visible/handle:scale-x-200",
          edge === "start" ? "origin-left" : "origin-right"
        )}
        style={{ left: place.line - place.grab }}
      />
      <div
        className={cn(
          "absolute bg-(--wf-brand) group-hover/handle:scale-125 group-focus-visible/handle:ring-2 group-focus-visible/handle:ring-ring group-data-dragging/handle:scale-125",
          edge === "start"
            ? "top-0 origin-top-left rounded-br-[3px]"
            : "bottom-0 origin-bottom-right rounded-tl-[3px]"
        )}
        style={{
          left: place.flag - place.grab,
          width: FLAG_WIDTH,
          height: FLAG_WIDTH,
        }}
      />
    </div>
  )
}

type WaveformProps = Omit<React.ComponentProps<"div">, "ref" | "children"> & {
  ref?: React.Ref<WaveformHandle>
  /** Min and max pairs, one pair per bucket: min, max, min, max. */
  peaks: WaveformPeaks
  /** Region start and end, 0 to 1. Set them to show draggable handles. */
  start?: number
  end?: number
  onStartChange?: (start: number) => void
  onEndChange?: (end: number) => void
  /** A handle drag or key hold is one gesture. */
  onGestureStart?: () => void
  onGestureEnd?: () => void
  /** The smallest region the handles can leave, 0 to 1. */
  minRegion?: number
  /** Formats a 0 to 1 position for screen readers, such as a time. */
  formatPosition?: (position: number) => string
  /**
   * Magnify the drawing so the loudest peak fills the height, by at most
   * `MAX_DISPLAY_GAIN`. On by default; a label in the corner says by how
   * much. Turn it off to draw full scale as the full height.
   */
  normalize?: boolean
  /** Waveform color as any CSS color. Defaults to `--wf-waveform`. */
  color?: string
  disabled?: boolean
}

const defaultFormatPosition = (position: number) => formatPercent(position, 1)

/** A waveform overview with optional region handles and a playhead. */
function Waveform({
  ref,
  className,
  peaks,
  start,
  end,
  onStartChange,
  onEndChange,
  onGestureStart,
  onGestureEnd,
  minRegion = 0.001,
  formatPosition = defaultFormatPosition,
  normalize = true,
  color = "var(--wf-waveform)",
  disabled = false,
  ...props
}: WaveformProps) {
  const rootRef = React.useRef<HTMLDivElement>(null)
  const canvasRef = React.useRef<HTMLCanvasElement>(null)
  const playheadRef = React.useRef<HTMLDivElement>(null)
  const redraw = React.useRef<(() => void) | null>(null)
  const gain = React.useMemo(
    () => (normalize ? waveformDisplayGain(peaks) : 1),
    [peaks, normalize]
  )
  const input = React.useRef({ peaks, gain, color })
  // The handles are placed in pixels, which takes the width.
  const [width, setWidth] = React.useState(0)

  React.useLayoutEffect(() => {
    input.current = { peaks, gain, color }
    redraw.current?.()
  }, [peaks, gain, color])

  React.useEffect(() => {
    const root = rootRef.current
    const canvas = canvasRef.current
    const context = canvas?.getContext("2d")
    if (!root || !canvas || !context) {
      return undefined
    }
    let size: CanvasSize | null = null
    const draw = () => {
      if (!size) {
        return
      }
      const { peaks, gain, color } = input.current
      const colors = resolveColors(root, {
        wave: color,
        line: "var(--wf-grid-line-strong)",
      })
      context.clearRect(0, 0, size.pixelWidth, size.pixelHeight)
      context.fillStyle = colors.line
      const line = Math.max(1, Math.round(size.dpr))
      context.fillRect(
        0,
        Math.floor((size.pixelHeight - line) / 2),
        size.pixelWidth,
        line
      )
      drawWaveform(
        context,
        peaks,
        size.pixelWidth,
        size.pixelHeight,
        colors.wave,
        gain
      )
    }
    redraw.current = draw
    const stopCanvas = observeCanvas(canvas, (next) => {
      size = next
      draw()
    })
    const stopTheme = subscribeTheme(draw)
    return () => {
      redraw.current = null
      stopCanvas()
      stopTheme()
    }
  }, [])

  const attachRoot = React.useCallback((root: HTMLDivElement | null) => {
    rootRef.current = root
    if (!root) {
      return undefined
    }
    // Measured as the element attaches, so the first paint has the handles
    // in place.
    setWidth(root.clientWidth)
    const canvas = canvasRef.current
    if (!canvas || typeof ResizeObserver === "undefined") {
      return undefined
    }
    // The canvas fills the root, and its content box is the exact width,
    // fractions of a pixel included.
    const observer = new ResizeObserver(([entry]) =>
      setWidth(entry.contentRect.width)
    )
    observer.observe(canvas)
    return () => observer.disconnect()
  }, [])

  React.useImperativeHandle(
    ref,
    () => ({
      setPlayhead(position) {
        const playhead = playheadRef.current
        if (!playhead) {
          return
        }
        if (position === null) {
          playhead.style.display = "none"
          return
        }
        const clamped = Math.min(1, Math.max(0, position))
        playhead.style.display = "block"
        playhead.style.left = `${(clamped * 100).toFixed(4)}%`
      },
    }),
    []
  )

  const hasRegion = start !== undefined || end !== undefined
  const from = Math.min(1, Math.max(0, start ?? 0))
  const to = Math.min(1, Math.max(from, end ?? 1))
  const gainDb = gainToDb(gain)

  return (
    <div
      ref={attachRoot}
      data-slot="waveform"
      data-disabled={disabled ? "" : undefined}
      className={cn(
        "relative h-20 w-full overflow-hidden rounded-sm bg-(--wf-meter-bg)/40 select-none data-disabled:opacity-50",
        className
      )}
      {...props}
    >
      <canvas
        ref={canvasRef}
        aria-hidden="true"
        className="absolute inset-0 block size-full"
      />
      {hasRegion ? (
        <>
          <div
            data-slot="waveform-shade"
            className="pointer-events-none absolute inset-y-0 left-0 bg-background/70"
            style={{ width: `${(from * 100).toFixed(4)}%` }}
          />
          <div
            data-slot="waveform-shade"
            className="pointer-events-none absolute inset-y-0 right-0 bg-background/70"
            style={{ width: `${((1 - to) * 100).toFixed(4)}%` }}
          />
        </>
      ) : null}
      <div
        ref={playheadRef}
        data-slot="waveform-playhead"
        aria-hidden="true"
        className="pointer-events-none absolute inset-y-0 hidden w-px bg-(--wf-playhead)"
      />
      {Math.abs(gainDb) > GAIN_LABEL_DB ? (
        <span
          data-slot="waveform-gain"
          title={message("Display gain {db}", { db: formatDb(gainDb, 0) })}
          className="absolute top-0.5 right-1 rounded-[2px] bg-background/70 px-0.5 text-[9px] leading-3 text-muted-foreground tabular-nums"
        >
          {formatDisplayGain(gain)}
        </span>
      ) : null}
      {start !== undefined ? (
        <RegionHandle
          edge="start"
          width={width}
          value={from}
          min={0}
          max={Math.max(0, to - minRegion)}
          label={t("Region start")}
          disabled={disabled}
          format={formatPosition}
          onValueChange={onStartChange}
          onGestureStart={onGestureStart}
          onGestureEnd={onGestureEnd}
        />
      ) : null}
      {end !== undefined ? (
        <RegionHandle
          edge="end"
          width={width}
          value={to}
          min={Math.min(1, from + minRegion)}
          max={1}
          label={t("Region end")}
          disabled={disabled}
          format={formatPosition}
          onValueChange={onEndChange}
          onGestureStart={onGestureStart}
          onGestureEnd={onGestureEnd}
        />
      ) : null}
    </div>
  )
}

export { Waveform }
export type { WaveformProps }
