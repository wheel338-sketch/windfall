import { createPointerFrame, type PointerFrame } from "@/lib/canvas"

import { wheelInput, type GridMetrics } from "./metrics"
import type { PlaylistSession, PointerInput } from "./session"
import { usePlaylistStore } from "./store"

type Binding = {
  /** Converts a pointer position to CSS pixels inside the grid. */
  localPoint(event: { clientX: number; clientY: number }): {
    x: number
    y: number
  }
  /** Gives the grid the keyboard, so the arrow keys and Delete reach it. */
  focus(): void
}

function inputOf(event: PointerEvent, frame: PointerFrame): PointerInput {
  return {
    ...frame.point(event),
    button: event.button,
    shift: event.shiftKey,
    mod: event.ctrlKey || event.metaKey,
    alt: event.altKey,
  }
}

/**
 * Feeds the canvas's pointer and wheel events to the session. Returns a
 * function that stops.
 */
export function attachPointer(
  element: HTMLElement,
  session: PlaylistSession,
  metrics: GridMetrics,
  binding: Binding
): () => void {
  // A press that selects a clip can change the layout around the grid.
  // The drag is measured from where the grid was when it began, so the
  // grid moving under a still pointer is never taken for a drag.
  const frame = createPointerFrame(binding.localPoint)

  const onPointerDown = (event: PointerEvent) => {
    binding.focus()
    // Stops the browser's own middle-button scrolling.
    if (event.button === 1) event.preventDefault()
    frame.hold(event)
    if (session.pointerDown(inputOf(event, frame))) {
      element.setPointerCapture(event.pointerId)
    } else {
      frame.release()
    }
  }
  const onPointerMove = (event: PointerEvent) => {
    session.pointerMove(inputOf(event, frame))
  }
  const onPointerUp = (event: PointerEvent) => {
    if (element.hasPointerCapture(event.pointerId)) {
      element.releasePointerCapture(event.pointerId)
    }
    const input = inputOf(event, frame)
    frame.release()
    void session.pointerUp(input)
  }
  const onPointerCancel = () => {
    frame.release()
    session.cancel()
  }
  const onPointerLeave = () => session.pointerLeave()
  const onMouseDown = (event: MouseEvent) => {
    if (event.button === 1) event.preventDefault()
  }
  const onContextMenu = (event: MouseEvent) => {
    // The right button deletes in every tool but Select, where the event
    // goes on to open the menu. Set to open the menu, it does so in every
    // tool but Playback, which seeks.
    const { tool, rightClickDeletes } = usePlaylistStore.getState()
    if (tool === "select" || (!rightClickDeletes && tool !== "playback"))
      return
    event.preventDefault()
    event.stopPropagation()
  }
  const onWheel = (event: WheelEvent) => {
    event.preventDefault()
    metrics.wheel(wheelInput(event, frame.point(event)))
  }

  element.addEventListener("pointerdown", onPointerDown)
  element.addEventListener("pointermove", onPointerMove)
  element.addEventListener("pointerup", onPointerUp)
  element.addEventListener("pointercancel", onPointerCancel)
  element.addEventListener("pointerleave", onPointerLeave)
  element.addEventListener("mousedown", onMouseDown)
  element.addEventListener("contextmenu", onContextMenu)
  // Not passive, or Ctrl+wheel would zoom the whole window.
  element.addEventListener("wheel", onWheel, { passive: false })
  return () => {
    element.removeEventListener("pointerdown", onPointerDown)
    element.removeEventListener("pointermove", onPointerMove)
    element.removeEventListener("pointerup", onPointerUp)
    element.removeEventListener("pointercancel", onPointerCancel)
    element.removeEventListener("pointerleave", onPointerLeave)
    element.removeEventListener("mousedown", onMouseDown)
    element.removeEventListener("contextmenu", onContextMenu)
    element.removeEventListener("wheel", onWheel)
  }
}
