import { logicalDelta, observePixelRatio } from "@/lib/ui-scale"
import {
  useEffect,
  useEffectEvent,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
} from "react"

import type { EqParams } from "@/bindings"
import { formatParam, type ParamBinder } from "@/features/params"
import { useHint } from "@/lib/store"
import { clamp } from "@/lib/units"
import { cn } from "@/lib/utils"

import { useDisplayCanvas } from "../use-display-canvas"
import { bandPoint, bandSpec, EQ_BANDS, type BandSpec } from "./bands"
import {
  bandResponseDb,
  EQ_BAND_IDS,
  eqResponseDb,
  responseGrid,
  type EqBandId,
} from "./eq-response"
import {
  frequencyLines,
  frequencyToX,
  gainLines,
  gainToY,
  plotRect,
  roundTo,
  xToFrequency,
  yToGain,
  type Plot,
} from "./geometry"
import { translatedText as t, message } from "@/lib/i18n"

/** Points each curve is worked out at, whatever the display's width. */
export const CURVE_POINTS = 256
/** Used until the display has been measured. */
const UNMEASURED = { width: 320, height: 160 }
/** A press that moves less than this is a click, not a drag. */
const DRAG_SLOP = 3
/** Pointer travel that doubles or halves Q while Alt is held. */
const Q_DRAG_PIXELS = 48
/** How long after the last wheel notch a wheel gesture is over. */
const WHEEL_IDLE_MS = 350

const COLORS: Record<
  "grid" | "gridStrong" | "label" | "sum" | EqBandId,
  string
> = {
  grid: "var(--wf-grid-line)",
  gridStrong: "var(--wf-grid-line-strong)",
  label: "var(--muted-foreground)",
  sum: "var(--foreground)",
  ...(Object.fromEntries(
    EQ_BANDS.map((band) => [band.id, band.color])
  ) as Record<EqBandId, string>),
}

type Curves = {
  /** X of each point in CSS pixels. */
  xs: Float32Array
  sum: Float32Array
  bands: Record<EqBandId, Float32Array | null>
}

function computeCurves(
  params: EqParams,
  sampleRate: number,
  width: number,
  plot: Plot
): Curves {
  const xs = new Float32Array(CURVE_POINTS)
  const frequencies = new Float64Array(CURVE_POINTS)
  // The curve runs past the ends of the axis to the edges of the display,
  // but a filter has no response at or above half the sample rate.
  const ceiling = 0.499 * sampleRate
  for (let index = 0; index < CURVE_POINTS; index += 1) {
    xs[index] = (index / (CURVE_POINTS - 1)) * width
    frequencies[index] = Math.min(ceiling, xToFrequency(xs[index], plot))
  }
  const grid = responseGrid(frequencies, sampleRate)
  const bands = {} as Curves["bands"]
  for (const id of EQ_BAND_IDS) {
    const band = params[id]
    const flat = !band.enabled || ("gainDb" in band && band.gainDb === 0)
    bands[id] = flat ? null : bandResponseDb(params, id, sampleRate, grid)
  }
  return { xs, sum: eqResponseDb(params, sampleRate, grid), bands }
}

function tracePath(
  context: CanvasRenderingContext2D,
  xs: Float32Array,
  gains: Float32Array,
  toY: (db: number) => number,
  height: number
) {
  context.beginPath()
  for (let index = 0; index < xs.length; index += 1) {
    // A steep cut reads hundreds of dB down, far outside any canvas.
    const y = clamp(toY(gains[index]), -height, 2 * height)
    if (index === 0) context.moveTo(xs[index], y)
    else context.lineTo(xs[index], y)
  }
}

function describe(bind: ParamBinder, spec: BandSpec, params: EqParams) {
  const point = bandPoint(params, spec.id)
  const parts = [formatParam(bind.info(spec.frequency), point.frequencyHz)]
  if (spec.gain) parts.push(formatParam(bind.info(spec.gain), point.gainDb))
  parts.push(`Q ${formatParam(bind.info(spec.q), point.q)}`)
  return parts
}

type EqDisplayProps = {
  /** What the controls show, which is ahead of the project during a drag. */
  params: EqParams
  sampleRate: number
  /** The gain the display shows either side of 0 dB. */
  range: number
  selected: EqBandId
  onSelect(band: EqBandId): void
  bind: ParamBinder
  /**
   * Drawn over the grid and under the nodes, filling the display: the
   * track's live spectrum. It takes no pointer events.
   */
  underlay?: ReactNode
  className?: string
}

/**
 * The equaliser's curve with a node for each band. Drag a node to set its
 * band's frequency and gain; hold Alt and drag up or down, or turn the
 * wheel, for Q; double-click to put the band back to its defaults. The node
 * of a band that is off is hollow, and a click switches the band on.
 */
export function EqDisplay({
  params,
  sampleRate,
  range,
  selected,
  onSelect,
  bind,
  underlay,
  className,
}: EqDisplayProps) {
  const root = useRef<HTMLDivElement>(null)
  const [measured, setMeasured] = useState<typeof UNMEASURED | null>(null)
  const [pointed, setPointed] = useState<EqBandId | null>(null)
  const size = measured ?? UNMEASURED
  const plot = useMemo(
    () => plotRect(size.width, size.height),
    [size.width, size.height]
  )
  const curves = useMemo(
    () => computeCurves(params, sampleRate, size.width, plot),
    [params, sampleRate, size.width, plot]
  )
  const hint = useHint(
    t("Drag a node for frequency and gain. Alt-drag or the wheel for Q. Double-click resets the band")
  )

  useLayoutEffect(() => {
    const element = root.current
    if (!element) return
    const measure = () => {
      const rect = element.getBoundingClientRect()
      const width = logicalDelta(rect.width)
      const height = logicalDelta(rect.height)
      if (width <= 0 || height <= 0) return
      setMeasured((current) =>
        current?.width === width && current.height === height
          ? current
          : { width, height }
      )
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    const stopPixelObserver = observePixelRatio(measure)
    return () => {
      observer.disconnect()
      stopPixelObserver()
    }
  }, [])

  const canvas = useDisplayCanvas(COLORS, (context, canvasSize, colors) => {
    const { width, height } = canvasSize
    const area = plotRect(width, height)
    const toY = (db: number) => gainToY(db, area, range)
    const crisp = (value: number) => Math.round(value) + 0.5
    const zero = crisp(toY(0))

    context.lineWidth = 1
    context.font = "9px 'Martian Mono Variable', ui-monospace, monospace"
    for (const line of frequencyLines()) {
      const x = crisp(frequencyToX(line.hz, area))
      context.strokeStyle = line.decade ? colors.gridStrong : colors.grid
      context.beginPath()
      context.moveTo(x, 0)
      context.lineTo(x, height - (line.label ? 13 : 0))
      context.stroke()
      if (line.label) {
        context.fillStyle = colors.label
        context.textAlign = "center"
        context.textBaseline = "alphabetic"
        context.fillText(line.label, x, height - 3)
      }
    }
    for (const line of gainLines(range)) {
      const y = crisp(toY(line.db))
      context.strokeStyle = line.db === 0 ? colors.gridStrong : colors.grid
      context.beginPath()
      context.moveTo(0, y)
      context.lineTo(width, y)
      context.stroke()
      if (line.label && line.db !== 0 && Math.abs(line.db) !== range) {
        context.fillStyle = colors.label
        context.textAlign = "left"
        context.textBaseline = "bottom"
        context.fillText(line.label, 3, y - 1)
      }
    }

    for (const id of EQ_BAND_IDS) {
      const gains = curves.bands[id]
      if (!gains) continue
      const chosen = id === selected
      tracePath(context, curves.xs, gains, toY, height)
      context.strokeStyle = colors[id]
      context.globalAlpha = chosen ? 0.95 : 0.5
      context.lineWidth = 1
      context.stroke()
      context.lineTo(width, zero)
      context.lineTo(0, zero)
      context.closePath()
      context.fillStyle = colors[id]
      context.globalAlpha = chosen ? 0.24 : 0.07
      context.fill()
    }

    context.globalAlpha = 1
    tracePath(context, curves.xs, curves.sum, toY, height)
    context.strokeStyle = colors.sum
    context.lineWidth = 1.75
    context.lineJoin = "round"
    context.stroke()
  })

  // One undo step each for a drag, a held key and a burst of the wheel.
  const dragGroup = bind.group("eq-node")
  const keyGroup = bind.group("eq-node-key")
  const wheelGroup = bind.group("eq-node-wheel")
  const drag = useRef<{
    band: EqBandId
    pointerId: number
    startX: number
    startY: number
    /** Where the pointer was at the move before this one. */
    lastY: number
    /** From the pointer to the node's centre, so the node does not jump. */
    offsetX: number
    offsetY: number
    moved: boolean
    wasEnabled: boolean
    /** Where Alt was pressed and the Q the band had then. */
    qFrom: { y: number; q: number } | null
  } | null>(null)
  const keyOpen = useRef(false)
  const wheelTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  function nodeAt(id: EqBandId) {
    const point = bandPoint(params, id)
    return {
      x: frequencyToX(point.frequencyHz, plot),
      y: gainToY(clamp(point.gainDb, -range, range), plot, range),
    }
  }

  function set(group: typeof dragGroup, id: string, value: number) {
    if (bind.value(id) !== value) group.set(id, value)
  }

  function onPointerDown(spec: BandSpec, event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0 || bind.disabled || !root.current) return
    event.preventDefault()
    event.currentTarget.focus({ preventScroll: true })
    event.currentTarget.setPointerCapture(event.pointerId)
    onSelect(spec.id)
    const rect = root.current.getBoundingClientRect()
    const node = nodeAt(spec.id)
    drag.current = {
      band: spec.id,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      lastY: event.clientY,
      offsetX: logicalDelta(event.clientX - rect.left) - node.x,
      offsetY: logicalDelta(event.clientY - rect.top) - node.y,
      moved: false,
      wasEnabled: bandPoint(params, spec.id).enabled,
      qFrom: null,
    }
  }

  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    const state = drag.current
    if (!state || state.pointerId !== event.pointerId || !root.current) return
    const spec = bandSpec(state.band)
    if (!state.moved) {
      const travel = Math.hypot(
        logicalDelta(event.clientX - state.startX),
        logicalDelta(event.clientY - state.startY)
      )
      if (travel < DRAG_SLOP) return
      state.moved = true
      dragGroup.onGestureStart()
      // Dragging the node of a band that is off switches it on.
      if (!state.wasEnabled) dragGroup.set(spec.enabled, 1)
    }
    const from = state.lastY
    state.lastY = event.clientY
    if (event.altKey) {
      // Measured from where the pointer was when Alt went down.
      state.qFrom ??= { y: from, q: bind.value(spec.q) }
      const octaves =
        logicalDelta(state.qFrom.y - event.clientY) / Q_DRAG_PIXELS
      set(dragGroup, spec.q, roundTo(state.qFrom.q * 2 ** octaves, 3))
      return
    }
    state.qFrom = null
    const rect = root.current.getBoundingClientRect()
    const x = clamp(
      logicalDelta(event.clientX - rect.left) - state.offsetX,
      plot.left,
      plot.left + plot.width
    )
    set(dragGroup, spec.frequency, roundTo(xToFrequency(x, plot), 3))
    if (spec.gain) {
      const y = logicalDelta(event.clientY - rect.top) - state.offsetY
      const gain = clamp(yToGain(y, plot, range), -range, range)
      set(dragGroup, spec.gain, Math.round(gain * 10) / 10)
    }
  }

  function endDrag(event: PointerEvent<HTMLDivElement>, clicked: boolean) {
    const state = drag.current
    if (!state || state.pointerId !== event.pointerId) return
    drag.current = null
    // The pointer may have let go somewhere else than over the node.
    if (!event.currentTarget.matches(":hover")) setPointed(null)
    if (state.moved) {
      dragGroup.onGestureEnd()
    } else if (clicked && !state.wasEnabled) {
      toggleBand(bandSpec(state.band), true)
    }
  }

  function toggleBand(spec: BandSpec, enabled: boolean) {
    dragGroup.onGestureStart()
    dragGroup.set(spec.enabled, enabled ? 1 : 0)
    dragGroup.onGestureEnd()
  }

  function resetBand(spec: BandSpec) {
    if (bind.disabled) return
    dragGroup.onGestureStart()
    for (const id of [
      spec.enabled,
      spec.frequency,
      spec.gain,
      spec.q,
      spec.slope,
    ]) {
      if (id !== null) set(dragGroup, id, bind.info(id).default)
    }
    dragGroup.onGestureEnd()
  }

  function endKeys() {
    if (!keyOpen.current) return
    keyOpen.current = false
    keyGroup.onGestureEnd()
  }

  function onKeyDown(spec: BandSpec, event: KeyboardEvent<HTMLDivElement>) {
    if (event.altKey || event.ctrlKey || event.metaKey || bind.disabled) return
    const fine = event.shiftKey
    const sign =
      event.key === "ArrowRight" ||
      event.key === "ArrowUp" ||
      event.key === "PageUp"
        ? 1
        : -1
    let id: string
    let value: number
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      id = spec.frequency
      value = roundTo(bind.value(id) * 2 ** (sign / (fine ? 48 : 12)), 4)
    } else if (
      (event.key === "ArrowUp" || event.key === "ArrowDown") &&
      spec.gain
    ) {
      id = spec.gain
      value = Math.round((bind.value(id) + sign * (fine ? 0.1 : 1)) * 10) / 10
    } else if (event.key === "PageUp" || event.key === "PageDown") {
      id = spec.q
      value = roundTo(bind.value(id) * 2 ** (sign / (fine ? 16 : 4)), 3)
    } else if (event.key === "Enter") {
      event.preventDefault()
      toggleBand(spec, bind.value(spec.enabled) < 0.5)
      return
    } else {
      return
    }
    event.preventDefault()
    if (!keyOpen.current) {
      keyOpen.current = true
      keyGroup.onGestureStart()
    }
    set(keyGroup, id, value)
  }

  const onWheel = useEffectEvent((event: WheelEvent) => {
    if (bind.disabled || !(event.target instanceof Element)) return
    const node = event.target.closest<HTMLElement>("[data-band]")
    const id = EQ_BAND_IDS.find((band) => band === node?.dataset.band)
    if (!id || event.ctrlKey) return
    // The wheel is the node's here, so the panel must not scroll with it.
    event.preventDefault()
    const spec = bandSpec(id)
    onSelect(id)
    if (wheelTimer.current === null) wheelGroup.onGestureStart()
    else clearTimeout(wheelTimer.current)
    wheelTimer.current = setTimeout(() => {
      wheelTimer.current = null
      wheelGroup.onGestureEnd()
    }, WHEEL_IDLE_MS)
    const q = bind.value(spec.q) * 2 ** (-event.deltaY / 400)
    set(wheelGroup, spec.q, roundTo(q, 3))
  })

  const endWheel = useEffectEvent(() => {
    if (wheelTimer.current === null) return
    clearTimeout(wheelTimer.current)
    wheelTimer.current = null
    wheelGroup.onGestureEnd()
  })

  useEffect(() => {
    const element = root.current
    if (!element) return
    const listener = (event: WheelEvent) => onWheel(event)
    // React listens to the wheel passively, which cannot stop the scroll.
    element.addEventListener("wheel", listener, { passive: false })
    return () => {
      element.removeEventListener("wheel", listener)
      endWheel()
    }
  }, [])

  const shown = pointed

  return (
    <div
      ref={root}
      role="group"
      aria-label={t("Equaliser curve")}
      data-slot="eq-display"
      className={cn(
        "relative h-40 touch-none overflow-hidden rounded-[5px] bg-(--wf-meter-bg) ring-1 ring-(--wf-grid-line-strong) select-none",
        className
      )}
      {...hint}
    >
      <canvas ref={canvas} className="absolute inset-0 size-full" />
      {underlay}
      {EQ_BANDS.map((spec) => {
        const point = bandPoint(params, spec.id)
        const at = nodeAt(spec.id)
        const text = describe(bind, spec, params)
        return (
          <div
            key={spec.id}
            role="slider"
            tabIndex={bind.disabled ? -1 : 0}
            aria-label={spec.name}
            aria-orientation="horizontal"
            aria-valuemin={bind.info(spec.frequency).min}
            aria-valuemax={bind.info(spec.frequency).max}
            aria-valuenow={point.frequencyHz}
            aria-valuetext={
              point.enabled ? text.join(", ") : message("Off, {text}", { text: text.join(", ") })
            }
            data-slot="eq-node"
            data-band={spec.id}
            data-enabled={point.enabled || undefined}
            data-selected={spec.id === selected || undefined}
            className="group/node absolute flex size-5 -translate-x-1/2 -translate-y-1/2 cursor-grab touch-none items-center justify-center rounded-full outline-none active:cursor-grabbing data-selected:z-10"
            style={
              { left: at.x, top: at.y, "--band": spec.color } as CSSProperties
            }
            onPointerDown={(event) => onPointerDown(spec, event)}
            onPointerMove={onPointerMove}
            onPointerUp={(event) => endDrag(event, true)}
            onPointerCancel={(event) => endDrag(event, false)}
            onLostPointerCapture={(event) => endDrag(event, false)}
            onDoubleClick={() => resetBand(spec)}
            onKeyDown={(event) => onKeyDown(spec, event)}
            onKeyUp={endKeys}
            onFocus={() => {
              onSelect(spec.id)
              setPointed(spec.id)
            }}
            onBlur={() => {
              endKeys()
              setPointed(null)
            }}
            onPointerEnter={() => setPointed(spec.id)}
            onPointerLeave={() => {
              if (!drag.current) setPointed(null)
            }}
          >
            <span
              className={cn(
                "block size-2.5 rounded-full border-[1.5px] border-(--band) transition-transform group-hover/node:scale-125 group-focus-visible/node:ring-2 group-focus-visible/node:ring-ring",
                "group-data-enabled/node:bg-(--band) group-data-enabled/node:shadow-[0_0_0_1.5px_var(--wf-meter-bg)]",
                "group-data-selected/node:scale-125 group-data-selected/node:shadow-[0_0_0_1.5px_var(--wf-meter-bg),0_0_0_3px_var(--band)]",
                !point.enabled && "bg-(--wf-meter-bg) opacity-80"
              )}
            />
          </div>
        )
      })}
      {shown && (
        <div
          aria-hidden
          data-slot="eq-readout"
          className="pointer-events-none absolute top-1 right-1.5 flex gap-1.5 rounded-[3px] bg-(--wf-meter-bg)/85 px-1 py-0.5 text-[10px] leading-none text-muted-foreground"
        >
          <span style={{ color: bandSpec(shown).color }}>
            {bandSpec(shown).name}
          </span>
          {describe(bind, bandSpec(shown), params).map((part) => (
            <span key={part} className="font-readout text-foreground">
              {part}
            </span>
          ))}
        </div>
      )}
    </div>
  )
}
