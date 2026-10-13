import { logicalDelta } from "@/lib/ui-scale"
import { useEffect, useRef } from "react"

import { useHint } from "@/lib/store/hint"
import { cn } from "@/lib/utils"

import { useSession } from "./context"
import type { PianoRollSession } from "./session"
import { ROW_COUNT, startAtThumbOffset, thumbGeometry } from "./view-math"
import { translatedText as t } from "@/lib/i18n"

const MIN_THUMB_PX = 28
/** The ends of the thumb that zoom when dragged. */
const HANDLE_PX = 8
/** The smallest share of the content a zoom drag can leave visible. */
const MIN_VISIBLE = 0.002

type Axis = "time" | "rows"
type Zone = "start" | "body" | "end"

type Drag = {
  zone: Zone
  /** Where in the thumb it was grabbed, in pixels from its start. */
  grab: number
  position: number
  start: number
  end: number
}

/** The visible part of the content as fractions of all of it. */
function visibleRange(session: PianoRollSession, axis: Axis) {
  const view = session.view
  if (!view) return { start: 0, end: 1 }
  const viewport = view.viewport
  if (axis === "time") {
    const total = Math.max(1, view.limits.contentTicks)
    const start = viewport.scrollTick / total
    return {
      start,
      end: Math.min(1, start + viewport.width / viewport.pxPerTick / total),
    }
  }
  const start = viewport.scrollRow / ROW_COUNT
  return {
    start,
    end: Math.min(1, start + viewport.height / viewport.rowHeight / ROW_COUNT),
  }
}

/**
 * A thin scrollbar for one axis of the grid. Dragging the bar scrolls;
 * dragging either end of it zooms, keeping the other end where it is.
 */
export function Scrollbar({ axis }: { axis: Axis }) {
  const session = useSession()
  const trackRef = useRef<HTMLDivElement>(null)
  const thumbRef = useRef<HTMLDivElement>(null)
  const drag = useRef<Drag | null>(null)
  const horizontal = axis === "time"
  const hint = useHint(
    horizontal
      ? t("Drag to scroll through time. Drag either end of the bar to zoom")
      : t("Drag to scroll through the keys. Drag either end of the bar to change the row height")
  )

  useEffect(() => {
    const sync = () => {
      const track = trackRef.current
      const thumb = thumbRef.current
      if (!track || !thumb) return
      const length = horizontal ? track.clientWidth : track.clientHeight
      const { start, end } = visibleRange(session, axis)
      const { offset, size } = thumbGeometry(start, end, length, MIN_THUMB_PX)
      if (horizontal) {
        thumb.style.width = `${size}px`
        thumb.style.transform = `translateX(${offset}px)`
      } else {
        thumb.style.height = `${size}px`
        thumb.style.transform = `translateY(${offset}px)`
      }
      track.setAttribute("aria-valuenow", String(Math.round(start * 100)))
    }
    sync()
    const stop = session.onView(sync)
    const track = trackRef.current
    const observer = new ResizeObserver(sync)
    if (track) observer.observe(track)
    return () => {
      stop()
      observer.disconnect()
    }
  }, [session, axis, horizontal])

  function measure(event: React.PointerEvent<HTMLDivElement>) {
    const bounds = event.currentTarget.getBoundingClientRect()
    return {
      length: logicalDelta(horizontal ? bounds.width : bounds.height),
      position: horizontal
        ? logicalDelta(event.clientX - bounds.left)
        : logicalDelta(event.clientY - bounds.top),
    }
  }

  function zoneAt(position: number, length: number): Zone | null {
    const { start, end } = visibleRange(session, axis)
    const { offset, size } = thumbGeometry(start, end, length, MIN_THUMB_PX)
    if (position < offset || position > offset + size) return null
    if (size < 3 * HANDLE_PX) return "body"
    if (position - offset < HANDLE_PX) return "start"
    if (offset + size - position < HANDLE_PX) return "end"
    return "body"
  }

  function onPointerDown(event: React.PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return
    const { length, position } = measure(event)
    const range = visibleRange(session, axis)
    const visible = range.end - range.start
    const { offset, size } = thumbGeometry(
      range.start,
      range.end,
      length,
      MIN_THUMB_PX
    )
    const zone = zoneAt(position, length)
    event.currentTarget.setPointerCapture(event.pointerId)
    if (zone === null) {
      // A click on the track brings the middle of the bar under the pointer.
      const start = startAtThumbOffset(
        position - size / 2,
        visible,
        length,
        MIN_THUMB_PX
      )
      session.scrollTo(axis, start)
      drag.current = { zone: "body", grab: size / 2, position, ...range }
      return
    }
    drag.current = { zone, grab: position - offset, position, ...range }
  }

  function onPointerMove(event: React.PointerEvent<HTMLDivElement>) {
    const { length, position } = measure(event)
    const current = drag.current
    if (!current) {
      const zone = zoneAt(position, length)
      const resize = horizontal ? "ew-resize" : "ns-resize"
      event.currentTarget.style.cursor =
        zone === "start" || zone === "end" ? resize : "default"
      return
    }
    const visible = current.end - current.start
    if (current.zone === "body") {
      const start = startAtThumbOffset(
        position - current.grab,
        visible,
        length,
        MIN_THUMB_PX
      )
      session.scrollTo(axis, start)
      return
    }
    const moved = (position - current.position) / Math.max(1, length)
    if (current.zone === "start") {
      const start = Math.min(
        current.end - MIN_VISIBLE,
        Math.max(0, current.start + moved)
      )
      session.setRange(axis, start, current.end)
    } else {
      const end = Math.max(
        current.start + MIN_VISIBLE,
        Math.min(1, current.end + moved)
      )
      session.setRange(axis, current.start, end)
    }
  }

  function onPointerEnd() {
    drag.current = null
  }

  return (
    <div
      ref={trackRef}
      role="scrollbar"
      aria-label={horizontal ? t("Scroll through time") : t("Scroll through keys")}
      aria-orientation={horizontal ? "horizontal" : "vertical"}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={0}
      className="group/scrollbar relative h-full w-full touch-none bg-chassis/40"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerEnd}
      onPointerCancel={onPointerEnd}
      {...hint}
    >
      <div
        ref={thumbRef}
        className={cn(
          "absolute rounded-full bg-foreground/25 group-hover/scrollbar:bg-foreground/40 group-active/scrollbar:bg-foreground/50",
          horizontal ? "inset-y-[2px] left-0" : "inset-x-[2px] top-0"
        )}
      />
    </div>
  )
}
