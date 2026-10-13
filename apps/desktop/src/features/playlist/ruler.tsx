import { logicalDelta, UI_SCALE_EVENT } from "@/lib/ui-scale"
import { useEffect, useRef } from "react"

import type {
  TimeSignature,
  MeterChange,
  TimelineMarker,
  TickRange,
} from "@/bindings"
import {
  observeCanvas,
  resolveColors,
  subscribeTheme,
  type CanvasSize,
} from "@/components/audio"
import {
  ContextActions,
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import {
  deviceTransform,
  deviceX,
  tickToX,
  visibleTicks,
  xToTick,
  type Viewport,
} from "@/lib/canvas"
import { ignoresSnap, MODIFIER_HINTS } from "@/lib/edit-modifiers"
import { useHint } from "@/lib/store/hint"
import { useProjectStore } from "@/lib/store/project"
import { usePlayhead } from "@/lib/store/realtime"
import { useSettings } from "@/lib/store/selectors"
import { useTransportStore } from "@/lib/store/transport"
import { ticksPerBar, ticksPerBeat } from "@/lib/time"
import { meterSegments, musicalPosition, rulerLabels } from "@/lib/timeline"
import { onProjectReplaced } from "@/lib/store/replaced"
import { TimelineGesture } from "./timeline-gesture"
import { useTimelineStore } from "./timeline-store"

import { snapNearest, songEnd } from "./edit"
import { markerJumps } from "./marker-jumps"
import { wheelInput, type GridMetrics } from "./metrics"
import { currentSnapTicks, seekSong, songTick, toggleLoopSong } from "./ops"
import { usePlaylistStore } from "./store"
import { translatedText as t, dynamicTitle, message, plainNumber } from "@/lib/i18n"

const RULER_COLORS = {
  ink: "var(--muted-foreground)",
  line: "var(--wf-grid-line-strong)",
  after: "color-mix(in oklch, var(--foreground) 7%, transparent)",
  end: "var(--muted-foreground)",
}

type RulerColors = typeof RULER_COLORS

/** Bar numbers get at least this much room each, in CSS pixels. */
const MIN_LABEL_SPACING = 46
const MIN_BEAT_SPACING = 7

/** How many bars one number stands for: 1 zoomed in, then 2, 4, 8 and so on. */
export function barsPerLabel(barPx: number): number {
  let bars = 1
  while (bars * barPx < MIN_LABEL_SPACING && bars < 4096) bars *= 2
  return bars
}

/**
 * Draws the bar numbers, the beat marks and the end of the song. Lines are
 * placed with the grid's own rounding, so they sit exactly above its lines.
 */
function drawRuler(
  ctx: CanvasRenderingContext2D,
  size: CanvasSize,
  viewport: Viewport,
  signature: TimeSignature,
  meters: readonly MeterChange[],
  markers: readonly TimelineMarker[],
  selection: TickRange | null,
  end: number,
  colors: RulerColors
): void {
  const { pixelWidth: width, pixelHeight: height, dpr } = size
  const transform = deviceTransform({ ...viewport, dpr })
  const lw = transform.lineWidth
  const ticks = visibleTicks(viewport)
  ctx.setTransform(1, 0, 0, 1, 0, 0)
  ctx.clearRect(0, 0, width, height)
  if (selection) {
    ctx.fillStyle = colors.after
    const left = deviceX(transform, selection.start)
    ctx.fillRect(left, 0, deviceX(transform, selection.end) - left, height)
  }

  const endX = deviceX(transform, end)
  if (end > 0 && endX < width) {
    ctx.fillStyle = colors.after
    ctx.fillRect(Math.max(0, endX), 0, width - Math.max(0, endX), height)
  }

  ctx.fillStyle = colors.line
  for (const segment of meterSegments(signature, meters)) {
    const beat = ticksPerBeat(segment.signature)
    const bar = ticksPerBar(segment.signature)
    if (beat * viewport.pxPerTick < MIN_BEAT_SPACING) continue
    const mark = Math.round(4 * dpr)
    const first =
      segment.start +
      Math.max(0, Math.floor((ticks.start - segment.start) / beat) * beat)
    for (
      let tick = first;
      tick <= ticks.end && tick < segment.end;
      tick += beat
    ) {
      if ((tick - segment.start) % bar === 0) continue
      ctx.fillRect(deviceX(transform, tick), height - mark, lw, mark)
    }
  }

  ctx.font = `500 ${Math.round(10 * dpr)}px "Inter Variable", system-ui, sans-serif`
  ctx.textBaseline = "middle"
  for (const label of rulerLabels(
    ticks.start,
    ticks.end,
    viewport.pxPerTick,
    signature,
    meters
  )) {
    const x = deviceX(transform, label.tick)
    ctx.fillStyle = colors.line
    ctx.fillRect(x, 0, lw, height)
    ctx.fillStyle = colors.ink
    ctx.fillText(
      String(label.bar),
      x + Math.round(4 * dpr),
      Math.round(height / 2 - dpr)
    )
  }

  for (const marker of markers) {
    const x = deviceX(transform, marker.tick)
    if (x < 0 || x >= width) continue
    ctx.fillStyle = colors.end
    ctx.fillRect(x, 0, lw * 2, height)
    ctx.fillText(marker.name, x + 4 * dpr, height - 5 * dpr, 140 * dpr)
  }

  if (end > 0 && endX >= 0 && endX < width) {
    // A small flag where the last clip ends.
    const flag = Math.round(6 * dpr)
    ctx.fillStyle = colors.end
    ctx.fillRect(endX, 0, 2 * lw, height)
    ctx.beginPath()
    ctx.moveTo(endX + 2 * lw, 0)
    ctx.lineTo(endX + 2 * lw + flag, flag / 2)
    ctx.lineTo(endX + 2 * lw, flag)
    ctx.fill()
  }
}

/** Moves the marker to a tick, or hides it when that is out of view. */
function placeMarker(
  marker: HTMLElement | null,
  shown: { current: number | null },
  metrics: GridMetrics,
  tick: number
): void {
  if (!marker) return
  const viewport = metrics.viewport
  const raw = tickToX(viewport, tick)
  const x =
    raw < -8 || raw > viewport.width + 8
      ? null
      : Math.round(raw * viewport.dpr) / viewport.dpr
  if (x === shown.current) return
  shown.current = x
  marker.style.visibility = x === null ? "hidden" : "visible"
  if (x !== null) marker.style.transform = `translateX(${x}px)`
}

/**
 * The bar ruler above the grid. Click or drag in it to move the song
 * position; it also shows where the song ends.
 *
 * The marker is the song position. In song mode it is the playhead and
 * moves while the song plays. In pattern mode the transport's playhead
 * runs inside the pattern instead, so the marker is dimmed and stays where
 * the song will start from.
 */
export function Ruler({ metrics }: { metrics: GridMetrics }) {
  const signature = useSettings().timeSignature
  const timeline = useProjectStore((state) => state.project.playlist.timeline)
  const tool = useTimelineStore((state) => state.tool)
  const selection = useTimelineStore((state) => state.draft ?? state.selection)
  const gesture = useRef<TimelineGesture | null>(null)
  const drag = useRef<"seek" | "region" | null>(null)
  const end = useProjectStore((state) => songEnd(state.project.playlist.clips))
  const mode = useTransportStore((state) => state.mode)
  const loop = useTransportStore((state) => state.loopSong)
  const cursor = usePlaylistStore((state) => state.cursorTick)
  const follow = usePlaylistStore((state) => state.follow)
  const toggleFollow = usePlaylistStore((state) => state.toggleFollow)
  const rootRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const markerRef = useRef<HTMLDivElement>(null)
  const markerX = useRef<number | null>(null)
  const lastSeek = useRef<number | null>(null)
  const hint = useHint(
    mode === "song"
      ? `Click or drag to move the playhead. ${MODIFIER_HINTS.free}`
      : t("Click to set where the song starts. The transport is looping the pattern, so the song is not playing")
  )

  usePlayhead((tick) => {
    if (useTransportStore.getState().mode !== "song") return
    placeMarker(markerRef.current, markerX, metrics, tick)
  })

  // The marker also has to move when the mode or the cursor changes.
  useEffect(() => {
    const place = () =>
      placeMarker(markerRef.current, markerX, metrics, songTick())
    place()
    return metrics.subscribe(place)
  }, [metrics, mode, cursor])

  useEffect(() => {
    const canvas = canvasRef.current
    const ctx = canvas?.getContext("2d")
    if (!canvas || !ctx) return
    let colors = resolveColors(canvas, RULER_COLORS)
    let size: CanvasSize | null = null
    let frame = 0
    const draw = () => {
      frame = 0
      if (size)
        drawRuler(
          ctx,
          size,
          metrics.viewport,
          signature,
          timeline?.meters ?? [],
          timeline?.markers ?? [],
          selection,
          end,
          colors
        )
    }
    // Several scroll events can arrive within one frame.
    const schedule = () => {
      if (frame === 0) frame = requestAnimationFrame(draw)
    }
    const stops = [
      observeCanvas(canvas, (next) => {
        size = next
        draw()
      }),
      metrics.subscribe(schedule),
      subscribeTheme(() => {
        colors = resolveColors(canvas, RULER_COLORS)
        schedule()
      }),
    ]
    return () => {
      cancelAnimationFrame(frame)
      for (const stop of stops) stop()
    }
  }, [metrics, signature, timeline, selection, end])

  useEffect(() => {
    const cancel = () => {
      gesture.current?.cancel()
      gesture.current = null
      drag.current = null
    }
    window.addEventListener("blur", cancel)
    window.addEventListener("resize", cancel)
    window.addEventListener(UI_SCALE_EVENT, cancel)
    const stops = [
      onProjectReplaced(cancel),
      metrics.subscribe(cancel),
      useTimelineStore.subscribe((state, prior) => {
        if (state.tool !== prior.tool) cancel()
      }),
      useProjectStore.subscribe((state, prior) => {
        if (state.revision !== prior.revision) cancel()
      }),
    ]
    return () => {
      cancel()
      window.removeEventListener("blur", cancel)
      window.removeEventListener("resize", cancel)
      window.removeEventListener(UI_SCALE_EVENT, cancel)
      stops.forEach((stop) => stop())
    }
  }, [metrics])

  useEffect(() => {
    const root = rootRef.current
    if (!root) return
    const onWheel = (event: WheelEvent) => {
      event.preventDefault()
      const bounds = root.getBoundingClientRect()
      const input = wheelInput(event, {
        x: logicalDelta(event.clientX - bounds.left),
        y: 0,
      })
      // Over the ruler a plain wheel scrolls time, which is what it shows.
      metrics.wheel({ ...input, shift: !input.mod })
    }
    root.addEventListener("wheel", onWheel, { passive: false })
    return () => root.removeEventListener("wheel", onWheel)
  }, [metrics])

  const seekAt = (event: React.PointerEvent<HTMLDivElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect()
    const tick = xToTick(
      metrics.viewport,
      logicalDelta(event.clientX - bounds.left)
    )
    const target = snapNearest(
      tick,
      ignoresSnap({ alt: event.altKey }) ? 0 : currentSnapTicks()
    )
    if (target === lastSeek.current) return
    lastSeek.current = target
    void seekSong(target)
  }

  const items: ContextItem[] = [
    { ...dynamicTitle("Jump to the start", {}), run: () => seekSong(0) },
    {
      ...dynamicTitle("Jump to the end of the song", {}),
      run: () => seekSong(end),
      disabled: end === 0,
    },
    ...markerJumps(timeline?.markers ?? []).map(({ title, tick }) => ({
      title,
      run: () => seekSong(tick),
    })),
    contextSeparator,
    "playlist.playSelection",
    "playlist.loopSelection",
    "playlist.zoomRegion",
    "playlist.exportRegion",
    "playlist.clearRegion",
    contextSeparator,
    "playlist.zoomToFit",
    {
      ...dynamicTitle(loop ? "Stop at the end of the song" : "Loop the song", {}),
      run: toggleLoopSong,
    },
    {
      ...dynamicTitle(follow ? "Stop following the playhead" : "Follow the playhead", {}),
      run: toggleFollow,
    },
  ]

  return (
    <ContextActions items={items}>
      <div
        ref={rootRef}
        role="slider"
        aria-label={t("Song position")}
        aria-valuemin={0}
        aria-valuemax={Math.max(end, 1)}
        aria-valuenow={cursor}
        aria-valuetext={message("Bar {bar}", { bar: plainNumber(musicalPosition(cursor, signature, timeline?.meters).bar) })}
        tabIndex={0}
        data-mode={mode}
        className="relative cursor-pointer overflow-hidden border-b bg-chassis/30"
        {...hint}
        onPointerDown={(event) => {
          if (event.button !== 0) return
          event.currentTarget.focus()
          event.currentTarget.setPointerCapture(event.pointerId)
          lastSeek.current = null
          const chosen = event.shiftKey ? "select" : tool
          drag.current = chosen === "seek" ? "seek" : "region"
          if (chosen === "seek") seekAt(event)
          else
            gesture.current = new TimelineGesture(
              metrics,
              logicalDelta(
                event.clientX - event.currentTarget.getBoundingClientRect().left
              ),
              chosen
            )
        }}
        onPointerMove={(event) => {
          if (event.currentTarget.hasPointerCapture(event.pointerId)) {
            if (gesture.current)
              gesture.current.update(
                logicalDelta(
                  event.clientX -
                    event.currentTarget.getBoundingClientRect().left
                )
              )
            else if (drag.current === "seek") seekAt(event)
          }
        }}
        onPointerUp={(event) => {
          gesture.current?.finish(
            logicalDelta(
              event.clientX - event.currentTarget.getBoundingClientRect().left
            )
          )
          gesture.current = null
          drag.current = null
          if (event.currentTarget.hasPointerCapture(event.pointerId)) {
            event.currentTarget.releasePointerCapture(event.pointerId)
          }
        }}
        onPointerCancel={() => {
          gesture.current?.cancel()
          gesture.current = null
          drag.current = null
        }}
        onLostPointerCapture={() => {
          gesture.current?.cancel()
          gesture.current = null
          drag.current = null
        }}
        onBlur={() => {
          hint.onBlur?.()
          gesture.current?.cancel()
          gesture.current = null
          drag.current = null
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape" && drag.current) {
            event.preventDefault()
            event.stopPropagation()
            gesture.current?.cancel()
            gesture.current = null
            drag.current = null
          }
          if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
            event.preventDefault()
            void seekSong(
              Math.max(
                0,
                cursor +
                  (event.key === "ArrowRight" ? 1 : -1) * currentSnapTicks()
              )
            )
          }
        }}
      >
        <canvas ref={canvasRef} className="absolute inset-0 size-full" />
        <div
          ref={markerRef}
          aria-hidden
          data-slot="song-marker"
          className="pointer-events-none absolute top-0 left-0 h-full w-0 in-data-[mode=pattern]:opacity-45"
        >
          <span className="absolute top-0 -left-[5px] border-x-[5px] border-t-[7px] border-x-transparent border-t-(--wf-playhead)" />
          <span className="absolute top-0 left-0 h-full w-px bg-(--wf-playhead)" />
        </div>
      </div>
    </ContextActions>
  )
}
