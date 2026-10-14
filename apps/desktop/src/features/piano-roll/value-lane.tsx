import { logicalDelta } from "@/lib/ui-scale"
import { useEffect, useRef } from "react"

import {
  deviceTransform,
  deviceX,
  resizedSpan,
  rgbaToCss,
  rgbFromInt,
  visibleRange,
  visibleTicks,
  xToTick,
  type Rgba,
} from "@/lib/canvas"
import { ticksPerBar } from "@/lib/time"

import { CanvasLayer, type LayerSize } from "./canvas-layer"
import { useSession } from "./context"
import { handleWheel } from "./grid-input"
import { showHint } from "./hint"
import { BarColumns } from "./lane-bars"
import {
  baselineY,
  LANE_KINDS,
  laneUpdates,
  laneValue,
  nearestBar,
  paintValues,
  scaleValues,
  valueAtY,
  yOfValue,
  type LaneKind,
} from "./lane-math"
import { fadePastEnd, shownLengthTicks } from "./overlays"
import type { PianoRollSession } from "./session"
import { usePianoRollStore } from "./store"
import { message, translatedText as t } from "@/lib/i18n"

/** More bars than this in view are drawn merged by pixel column. */
const MAX_SEPARATE_BARS = 3000
const plainColumns = new BarColumns()
const chosenColumns = new BarColumns()

/** How close to a bar, in pixels, a press has to be to grab it. */
const GRAB_PX = 5
const GUIDES: Record<LaneKind, { value: number; strong: boolean }[]> = {
  velocity: [
    { value: 1, strong: true },
    { value: 0.75, strong: false },
    { value: 0.5, strong: false },
    { value: 0.25, strong: false },
    { value: 0, strong: true },
  ],
  pan: [
    { value: 1, strong: false },
    { value: 0, strong: true },
    { value: -1, strong: false },
  ],
  release: [{ value: 1, strong: false }, { value: 0.5, strong: true }, { value: 0, strong: false }],
  finePitchCents: [{ value: 1200, strong: false }, { value: 0, strong: true }, { value: -1200, strong: false }],
  modulationX: [{ value: 1, strong: false }, { value: 0.5, strong: true }, { value: 0, strong: false }],
  modulationY: [{ value: 1, strong: false }, { value: 0.5, strong: true }, { value: 0, strong: false }],
}

const HINTS: Record<LaneKind, string> = {
  get velocity() { return t("Drag to set velocity; drag across bars to paint them. With notes selected only they change, and dragging one of them scales them together") },
  get pan() { return t("Drag to set note pan; drag across bars to paint them. With notes selected only they change, and dragging one of them shifts them together") },
  get release() { return t("Paint each note's release amount. The midpoint preserves the instrument's release time; lower values shorten it and higher values lengthen it") },
  get finePitchCents() { return t("Paint fine pitch in cents around the note's MIDI key. Selected notes keep their pitch differences when shifted together") },
  get modulationX() { return t("Paint per-note modulation X, which offsets cutoff in the built-in voices. The midpoint applies no offset") },
  get modulationY() { return t("Paint per-note modulation Y, which offsets resonance in the built-in voices. The midpoint applies no offset") },
}

type Stroke = {
  kind: LaneKind
  /** Values the drag has set so far, by note id. Shown until it is committed. */
  values: Map<number, number>
  mode: "paint" | "scale"
  last: { tick: number; value: number }
  grabbed: number
  only: ReadonlySet<number> | undefined
}

function paintLane(
  ctx: CanvasRenderingContext2D,
  size: LayerSize,
  session: PianoRollSession,
  kind: LaneKind,
  color: Rgba,
  values: ReadonlyMap<number, number> | null
) {
  const view = session.view
  const context = session.editor.context
  if (!view || !context) return
  const { viewport, theme } = view
  const transform = deviceTransform({ ...viewport, dpr: size.dpr })
  const { editor } = session
  const { dpr, width, height } = size
  const cssHeight = height / dpr
  const lw = transform.lineWidth

  for (const guide of GUIDES[kind]) {
    const y = Math.round(yOfValue(guide.value, cssHeight, kind) * dpr)
    ctx.fillStyle = rgbaToCss(guide.strong ? theme.gridBar : theme.gridBeat)
    ctx.fillRect(0, y, width, lw)
  }

  const bar = ticksPerBar(context.pattern.signature)
  let barsPerLine = 1
  while (barsPerLine * bar * viewport.pxPerTick < 12) barsPerLine *= 2
  const ticks = visibleTicks(viewport)
  const spacing = barsPerLine * bar
  ctx.fillStyle = rgbaToCss(theme.gridBar)
  for (
    let tick = Math.max(0, Math.floor(ticks.start / spacing) * spacing);
    tick <= ticks.end;
    tick += spacing
  ) {
    ctx.fillRect(deviceX(transform, tick), 0, lw, height)
  }

  const items = editor.items
  const batch = items.batch
  const notes = editor.notes
  const preview = editor.drag
  const reach = preview
    ? Math.abs(preview.ticks) + Math.abs(preview.resize?.start ?? 0)
    : 0
  const range = visibleRange(items, ticks.start - reach, ticks.end + reach)
  const base = Math.round(baselineY(cssHeight, kind) * dpr)
  const stem = Math.max(lw, Math.round(2 * dpr))
  const capWidth = Math.round(7 * dpr)
  const capHeight = Math.round(3 * dpr)
  // With this many bars in view they are merged by pixel column.
  const merge = range.last - range.first > MAX_SEPARATE_BARS
  const plain: number[] = []
  const chosen: number[] = []
  if (merge) {
    plainColumns.reset(width)
    chosenColumns.reset(width)
  }

  for (let i = range.first; i < range.last; i++) {
    const note = notes[i]
    const selected = batch.isSelected(i)
    let start = note.start
    if (selected && preview) {
      if (preview.resize) {
        start = resizedSpan(
          start,
          note.length,
          preview.resize.start,
          preview.resize.end,
          preview.resize.minLength
        ).start
      }
      start += preview.ticks
    }
    const x = deviceX(transform, start)
    if (x < -capWidth || x > width) continue
    const value = values?.get(note.id) ?? laneValue(note, kind)
    const y = Math.round(yOfValue(value, cssHeight, kind) * dpr)
    if (merge) (selected ? chosenColumns : plainColumns).add(x, y, base)
    else (selected ? chosen : plain).push(x, y)
  }

  const drawBars = (bars: number[], fill: string) => {
    ctx.fillStyle = fill
    for (let k = 0; k < bars.length; k += 2) {
      const x = bars[k]
      const y = bars[k + 1]
      const top = Math.min(y, base)
      ctx.fillRect(x, top, stem, Math.max(lw, Math.abs(y - base)))
      ctx.fillRect(x, y <= base ? y : y - capHeight, capWidth, capHeight)
    }
  }
  const drawColumns = (columns: BarColumns, fill: string) => {
    ctx.fillStyle = fill
    columns.forEach((x, top, bottom) => {
      ctx.fillRect(x, top, stem, Math.max(lw, bottom - top))
      if (top < base) ctx.fillRect(x, top, capWidth, capHeight)
      if (bottom > base) {
        ctx.fillRect(x, bottom - capHeight, capWidth, capHeight)
      }
    })
  }
  if (merge) {
    drawColumns(plainColumns, rgbaToCss(color))
    drawColumns(chosenColumns, rgbaToCss(theme.selectionBorder))
  } else {
    drawBars(plain, rgbaToCss(color))
    drawBars(chosen, rgbaToCss(theme.selectionBorder))
  }

  const endX = deviceX(transform, shownLengthTicks(session))
  if (endX < width) {
    const from = Math.max(0, endX)
    fadePastEnd(ctx, theme, from, width - from, height)
  }
}

type ValueLaneProps = {
  kind: LaneKind
  /** The channel's color as 0xRRGGBB. */
  color: number
}

/**
 * The strip under the grid: one bar per note at its start, for velocity or
 * pan. A drag sets values as a preview and commits one command on release.
 */
export function ValueLane({ kind, color }: ValueLaneProps) {
  const session = useSession()
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const layerRef = useRef<CanvasLayer | null>(null)
  const stroke = useRef<Stroke | null>(null)
  const latest = useRef({ kind, color })

  useEffect(() => {
    latest.current = { kind, color }
    layerRef.current?.invalidate()
  }, [kind, color])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const layer = new CanvasLayer(canvas, (ctx, size) =>
      paintLane(
        ctx,
        size,
        session,
        latest.current.kind,
        rgbFromInt(latest.current.color),
        stroke.current?.kind === latest.current.kind
          ? stroke.current.values
          : null
      )
    )
    layerRef.current = layer
    const invalidate = () => layer.invalidate()
    const stop = [
      session.onView(invalidate),
      session.editor.subscribe((event) => {
        if (event !== "hover") invalidate()
      }),
    ]
    const onWheel = (event: WheelEvent) => {
      const bounds = canvas.getBoundingClientRect()
      handleWheel(
        session,
        event,
        { x: logicalDelta(event.clientX - bounds.left), y: 0 },
        { time: true, rows: false }
      )
    }
    canvas.addEventListener("wheel", onWheel, { passive: false })
    return () => {
      for (const off of stop) off()
      canvas.removeEventListener("wheel", onWheel)
      layer.destroy()
      layerRef.current = null
    }
  }, [session])

  function locate(event: React.PointerEvent<HTMLCanvasElement>) {
    const view = session.view
    if (!view) return null
    const bounds = event.currentTarget.getBoundingClientRect()
    return {
      tick: xToTick(view.viewport, logicalDelta(event.clientX - bounds.left)),
      value: valueAtY(
        logicalDelta(event.clientY - bounds.top),
        logicalDelta(bounds.height),
        kind
      ),
      reach: GRAB_PX / view.viewport.pxPerTick,
    }
  }

  function onPointerDown(event: React.PointerEvent<HTMLCanvasElement>) {
    if (event.button !== 0) return
    const at = locate(event)
    if (!at) return
    session.focusGrid()
    const { editor } = session
    const selection = editor.selection
    const notes = editor.notes
    const grabbed = nearestBar(notes, at.tick, at.reach, selection)
    const scaling =
      grabbed !== null && selection.has(grabbed.id) && selection.size > 1
    const next: Stroke = {
      kind,
      values: new Map(),
      mode: scaling ? "scale" : "paint",
      last: { tick: at.tick, value: at.value },
      grabbed: grabbed ? laneValue(grabbed, kind) : 0,
      only: selection.size > 0 ? selection : undefined,
    }
    stroke.current = next
    event.currentTarget.setPointerCapture(event.pointerId)
    if (scaling) {
      next.values = scaleValues(
        editor.selectedNotes(),
        kind,
        next.grabbed,
        at.value
      )
    } else {
      // A press paints the bar under it; a drag paints what it passes.
      paintValues(
        notes,
        kind,
        { tick: at.tick, value: at.value },
        { tick: at.tick, value: at.value },
        at.reach,
        next.values,
        next.only
      )
    }
    layerRef.current?.invalidate()
  }

  function onPointerMove(event: React.PointerEvent<HTMLCanvasElement>) {
    const current = stroke.current
    const at = locate(event)
    if (!current || !at) return
    const { editor } = session
    if (current.mode === "scale") {
      current.values = scaleValues(
        editor.selectedNotes(),
        current.kind,
        current.grabbed,
        at.value
      )
    } else {
      paintValues(
        editor.notes,
        current.kind,
        current.last,
        { tick: at.tick, value: at.value },
        at.reach,
        current.values,
        current.only
      )
    }
    current.last = { tick: at.tick, value: at.value }
    layerRef.current?.invalidate()
  }

  function onPointerUp() {
    const current = stroke.current
    if (!current) return
    const { editor } = session
    const updates = laneUpdates(editor.notes, current.kind, current.values)
    const clear = () => {
      if (stroke.current === current) stroke.current = null
      layerRef.current?.invalidate()
    }
    if (updates.length === 0) {
      clear()
      return
    }
    if (current.kind === "velocity") {
      const { lastLength, rememberNote } = usePianoRollStore.getState()
      rememberNote(lastLength, current.last.value)
    }
    // The painted values stay on screen until the project has them.
    void editor
      .update(
        editor.notes,
        updates,
        `Change note ${LANE_KINDS.find((item) => item.id === current.kind)?.label.toLowerCase() ?? current.kind}`
      )
      .finally(clear)
  }

  function onPointerCancel() {
    stroke.current = null
    layerRef.current?.invalidate()
  }

  return (
    <canvas
      ref={canvasRef}
      aria-label={message("Note {findLabel} values", { findLabel: LANE_KINDS.find((item) => item.id === kind)?.label.toLowerCase() ?? "" })}
      className="block h-full w-full cursor-crosshair touch-none"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerCancel}
      onPointerEnter={() => showHint(HINTS[kind])}
      onPointerLeave={() => showHint(null)}
    />
  )
}
