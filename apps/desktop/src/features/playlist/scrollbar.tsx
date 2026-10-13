import { logicalDelta } from "@/lib/ui-scale"
import { useEffect, useRef } from "react"

import { clamp } from "@/lib/units"
import { cn } from "@/lib/utils"

import { physicalLimits } from "./row-geometry"
import type { GridMetrics } from "./metrics"
import { translatedText as t } from "@/lib/i18n"

const MIN_THUMB_PX = 28

type Extent = {
  /** Where the view starts, how much it shows and how much there is, in one unit. */
  start: number
  visible: number
  total: number
}

/** Where the thumb sits on a track `trackPx` long. Null when nothing can scroll. */
export function thumbBox(
  extent: Extent,
  trackPx: number
): { offset: number; size: number } | null {
  if (extent.total <= 0 || extent.visible >= extent.total || trackPx <= 0) {
    return null
  }
  const size = Math.min(
    trackPx,
    Math.max(MIN_THUMB_PX, (extent.visible / extent.total) * trackPx)
  )
  const travel = trackPx - size
  const range = extent.total - extent.visible
  return { offset: clamp(extent.start / range, 0, 1) * travel, size }
}

/** The scroll position a thumb dragged to `offset` pixels stands for. */
export function startForThumb(
  extent: Extent,
  trackPx: number,
  offset: number
): number {
  const box = thumbBox(extent, trackPx)
  if (!box) return 0
  const travel = trackPx - box.size
  if (travel <= 0) return 0
  return clamp(offset / travel, 0, 1) * (extent.total - extent.visible)
}

function extentOf(metrics: GridMetrics, axis: "time" | "tracks"): Extent {
  const viewport = metrics.viewport
  const limits = metrics.limits
  return axis === "time"
    ? {
        start: viewport.scrollTick,
        visible: viewport.width / viewport.pxPerTick,
        total: limits.contentTicks,
      }
    : {
        start: viewport.scrollRow,
        visible: viewport.height / viewport.rowHeight,
        total: physicalLimits(viewport, limits.rowCount),
      }
}

/**
 * A thin scrollbar for the grid. The grid is a canvas with no scrolling of
 * its own, so this reads and sets its viewport. The thumb is moved without
 * rendering.
 */
export function Scrollbar({
  metrics,
  axis,
}: {
  metrics: GridMetrics
  axis: "time" | "tracks"
}) {
  const trackRef = useRef<HTMLDivElement>(null)
  const thumbRef = useRef<HTMLDivElement>(null)
  const grab = useRef<{ pointer: number; offset: number } | null>(null)
  const horizontal = axis === "time"

  const trackPx = () => {
    const track = trackRef.current
    if (!track) return 0
    return horizontal ? track.clientWidth : track.clientHeight
  }

  const scrollTo = (start: number) => {
    const viewport = metrics.viewport
    metrics.setViewport(
      horizontal
        ? { ...viewport, scrollTick: start }
        : { ...viewport, scrollRow: start }
    )
  }

  useEffect(() => {
    const place = () => {
      const track = trackRef.current
      const thumb = thumbRef.current
      if (!track || !thumb) return
      const extent = extentOf(metrics, axis)
      const length = horizontal ? track.clientWidth : track.clientHeight
      const box = thumbBox(extent, length)
      thumb.style.visibility = box ? "visible" : "hidden"
      if (!box) return
      if (horizontal) {
        thumb.style.width = `${box.size}px`
        thumb.style.transform = `translateX(${box.offset}px)`
      } else {
        thumb.style.height = `${box.size}px`
        thumb.style.transform = `translateY(${box.offset}px)`
      }
      track.setAttribute(
        "aria-valuenow",
        String(
          Math.round((extent.start / (extent.total - extent.visible)) * 100)
        )
      )
    }
    place()
    return metrics.subscribe(place)
  }, [metrics, axis, horizontal])

  const along = (event: React.PointerEvent) => {
    const bounds = trackRef.current?.getBoundingClientRect()
    if (!bounds) return 0
    return horizontal
      ? logicalDelta(event.clientX - bounds.left)
      : logicalDelta(event.clientY - bounds.top)
  }

  return (
    <div
      ref={trackRef}
      role="scrollbar"
      aria-label={horizontal ? t("Scroll along the song") : t("Scroll the tracks")}
      aria-orientation={horizontal ? "horizontal" : "vertical"}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={0}
      className={cn(
        "relative bg-chassis/30",
        horizontal ? "border-t" : "border-l"
      )}
      onPointerDown={(event) => {
        if (event.button !== 0) return
        const extent = extentOf(metrics, axis)
        const box = thumbBox(extent, trackPx())
        if (!box) return
        const at = along(event)
        const onThumb = at >= box.offset && at <= box.offset + box.size
        // A press beside the thumb brings its middle under the pointer.
        const offset = onThumb ? at - box.offset : box.size / 2
        if (!onThumb) scrollTo(startForThumb(extent, trackPx(), at - offset))
        grab.current = { pointer: event.pointerId, offset }
        event.currentTarget.setPointerCapture(event.pointerId)
      }}
      onPointerMove={(event) => {
        const held = grab.current
        if (!held || held.pointer !== event.pointerId) return
        scrollTo(
          startForThumb(
            extentOf(metrics, axis),
            trackPx(),
            along(event) - held.offset
          )
        )
      }}
      onPointerUp={() => {
        grab.current = null
      }}
      onPointerCancel={() => {
        grab.current = null
      }}
    >
      <div
        ref={thumbRef}
        className={cn(
          "absolute rounded-full bg-foreground/20 hover:bg-foreground/35",
          horizontal ? "top-0.5 bottom-0.5 left-0" : "top-0 right-0.5 left-0.5"
        )}
      />
    </div>
  )
}
