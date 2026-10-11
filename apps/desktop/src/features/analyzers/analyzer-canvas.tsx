import { useEffect, useRef } from "react"

import {
  observeCanvas,
  resolveColors,
  subscribeTheme,
  type CanvasSize,
} from "@/components/audio"
import { cn } from "@/lib/utils"

import { useAnalyzerFrames } from "./feed"
import type { AnalyzerFrame } from "./types"

type Drawing = {
  context: CanvasRenderingContext2D | null
  size: CanvasSize | null
  colors: AnalyzerColors | null
  frame: AnalyzerFrame | null
  painted: number
  paint: AnalyzerCanvasProps["paint"]
}

function redraw(state: Drawing, element: HTMLCanvasElement | null) {
  const { context, size, colors, frame } = state
  if (element) {
    // Says, without a canvas to look at, that a frame reached the view.
    state.painted += 1
    element.dataset.painted = String(state.painted)
    element.dataset.live = frame ? "true" : "false"
    const spectrum = frame?.spectrum
    let strongest = 0
    const power = (bin: number) => (spectrum?.left[bin] ?? 0) + (spectrum?.right[bin] ?? 0)
    if (spectrum && (spectrum.left.length || spectrum.right.length)) {
      for (let bin = 1; bin < Math.max(spectrum.left.length, spectrum.right.length); bin += 1) {
        if (power(bin) > power(strongest)) strongest = bin
      }
      if (power(strongest) > 0) element.dataset.peakHz = String(strongest * spectrum.binHz)
      else delete element.dataset.peakHz
    } else delete element.dataset.peakHz
    element.dataset.sampleRate = frame ? String(frame.clock.sampleRate) : ""
    element.dataset.pdcFrames = frame ? String(frame.clock.pdcFrames) : ""
    element.dataset.epoch = frame?.clock.epoch ?? ""
    element.dataset.invalid = frame ? String(frame.levels[0].invalid + frame.levels[1].invalid) : ""
    element.dataset.subscription = frame?.ticket.subscription ?? ""
    element.dataset.sequence = frame?.sequence ?? ""
    element.dataset.skippedEvents = frame?.counters.skippedEvents ?? ""
  }
  if (!context || !size || !colors) return
  context.setTransform(size.dpr, 0, 0, size.dpr, 0, 0)
  context.clearRect(0, 0, size.width, size.height)
  state.paint(context, size, colors, frame)
}

/** The colors every analyzer display draws with, from the theme. */
export const ANALYZER_COLORS = {
  grid: "var(--wf-grid-line)",
  gridStrong: "var(--wf-grid-line-strong)",
  label: "var(--muted-foreground)",
  trace: "var(--wf-brand)",
  second: "var(--wf-waveform)",
  low: "var(--wf-meter-low)",
  mid: "var(--wf-meter-mid)",
  high: "var(--wf-meter-high)",
}

export type AnalyzerColors = Record<keyof typeof ANALYZER_COLORS, string>

export type AnalyzerPainter = (
  context: CanvasRenderingContext2D,
  size: CanvasSize,
  colors: AnalyzerColors,
  frame: AnalyzerFrame | null
) => void

type AnalyzerCanvasProps = {
  slot: string
  /** What the picture shows, for someone who cannot see it. */
  label: string
  /** Draws the whole canvas, already cleared and scaled to CSS pixels. */
  paint: AnalyzerPainter
  /** Adds one frame to a history the painter draws from. */
  onFrame?(frame: AnalyzerFrame): void
  /** Forgets that history. */
  onClear?(): void
  className?: string
}

/**
 * A canvas that draws a slot's analyzer frames. It repaints when a frame
 * arrives or is cleared, when it is resized and when the theme changes, at
 * most once per animation frame, and never through React state. Without a
 * frame it paints its empty state, so nothing stale stays on screen.
 */
export function AnalyzerCanvas({
  slot,
  label,
  paint,
  onFrame,
  onClear,
  className,
}: AnalyzerCanvasProps) {
  const canvas = useRef<HTMLCanvasElement>(null)
  const drawing = useRef<Drawing>({
    context: null,
    size: null,
    colors: null,
    frame: null,
    painted: 0,
    paint,
  })
  // Frames arrive outside React, so the painter they use is kept current
  // here rather than captured by whichever render subscribed.
  useEffect(() => {
    drawing.current.paint = paint
  })
  const draw = () => redraw(drawing.current, canvas.current)

  useEffect(() => {
    const element = canvas.current
    if (!element) return
    const state = drawing.current
    state.context = element.getContext("2d")
    state.colors = resolveColors(element, ANALYZER_COLORS)
    const stopObserving = observeCanvas(element, (size) => {
      state.size = size
      draw()
    })
    const stopTheme = subscribeTheme(() => {
      state.colors = resolveColors(element, ANALYZER_COLORS)
      draw()
    })
    return () => {
      stopObserving()
      stopTheme()
    }
  }, [])

  useAnalyzerFrames(slot, {
    onFrame,
    onClear,
    paint(frame) {
      drawing.current.frame = frame
      draw()
    },
  })

  return (
    <canvas
      ref={canvas}
      role="img"
      aria-label={label}
      data-slot="analyzer-canvas"
      data-live="false"
      className={cn("block size-full", className)}
    />
  )
}
