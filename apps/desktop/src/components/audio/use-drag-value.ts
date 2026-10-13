// SPDX-License-Identifier: MIT
import { logicalDelta } from "@/lib/ui-scale"
import * as React from "react"

import { parseNumber } from "./units"
import { message } from "@/lib/i18n"

/** Maps a value to a 0 to 1 control position and back. */
export type ValueScale = {
  toNormalized: (value: number, min: number, max: number) => number
  fromNormalized: (normalized: number, min: number, max: number) => number
}

export type ValueScaleOption = "linear" | "log" | ValueScale

export const linearScale: ValueScale = {
  toNormalized: (value, min, max) =>
    max === min ? 0 : (value - min) / (max - min),
  fromNormalized: (normalized, min, max) => min + normalized * (max - min),
}

/** Equal travel per octave, for frequencies and times. Needs `min` above 0. */
export const logScale: ValueScale = {
  toNormalized: (value, min, max) =>
    Math.log(Math.max(value, min) / min) / Math.log(max / min),
  fromNormalized: (normalized, min, max) => min * (max / min) ** normalized,
}

/** More travel at the low end for exponents above 1. */
export function powerScale(exponent: number): ValueScale {
  return {
    toNormalized: (value, min, max) =>
      max === min ? 0 : ((value - min) / (max - min)) ** (1 / exponent),
    fromNormalized: (normalized, min, max) =>
      min + normalized ** exponent * (max - min),
  }
}

export function resolveScale(scale: ValueScaleOption = "linear"): ValueScale {
  if (scale === "linear") {
    return linearScale
  }
  if (scale === "log") {
    return logScale
  }
  return scale
}

export function clampValue(value: number, min: number, max: number): number {
  if (Number.isNaN(value)) {
    return min
  }
  return Math.min(max, Math.max(min, value))
}

function clamp01(value: number): number {
  return clampValue(value, 0, 1)
}

function decimalsOf(value: number): number {
  const text = String(value)
  const exponent = /e-(\d+)$/.exec(text)
  if (exponent) {
    return Math.min(12, Number(exponent[1]))
  }
  const dot = text.indexOf(".")
  return dot < 0 ? 0 : Math.min(12, text.length - dot - 1)
}

/** Clamps to the range and, when `step` is set, to `min + k * step`. */
export function snapValue(
  value: number,
  min: number,
  max: number,
  step?: number
): number {
  const clamped = clampValue(value, min, max)
  if (!step || step <= 0) {
    return clamped
  }
  const snapped = min + Math.round((clamped - min) / step) * step
  // Rounding to the step's own precision removes float dust (0.1 + 0.2).
  const digits = Math.max(decimalsOf(step), decimalsOf(min))
  return clampValue(Number(snapped.toFixed(digits)), min, max)
}

export function valueToNormalized(
  value: number,
  min: number,
  max: number,
  scale?: ValueScaleOption
): number {
  return clamp01(
    resolveScale(scale).toNormalized(clampValue(value, min, max), min, max)
  )
}

export function normalizedToValue(
  normalized: number,
  min: number,
  max: number,
  scale?: ValueScaleOption
): number {
  return clampValue(
    resolveScale(scale).fromNormalized(clamp01(normalized), min, max),
    min,
    max
  )
}

export type StepMode = "normal" | "fine" | "coarse"

export type ValueRange = {
  min: number
  max: number
  step?: number
  keyStep?: number
  scale?: ValueScaleOption
}

const TRAVEL_PER_STEP: Record<StepMode, number> = {
  fine: 0.001,
  normal: 0.01,
  coarse: 0.1,
}

/**
 * One keyboard or wheel step. With a `step` the value moves by `keyStep`
 * (which defaults to the step), by one step in fine mode and by ten key
 * steps in coarse mode. Without a step it moves by a share of the control's
 * travel, so a tapered control steps evenly along its taper.
 */
export function stepValue(
  value: number,
  direction: number,
  mode: StepMode,
  { min, max, step, keyStep, scale }: ValueRange
): number {
  if (step && step > 0) {
    const normal = Math.max(step, keyStep ?? step)
    const size =
      mode === "fine" ? step : mode === "coarse" ? normal * 10 : normal
    return snapValue(value + direction * size, min, max, step)
  }
  const position = valueToNormalized(value, min, max, scale)
  return normalizedToValue(
    position + direction * TRAVEL_PER_STEP[mode],
    min,
    max,
    scale
  )
}

/** Share of the travel covered by a pointer movement of `pixels`. */
export function dragTravel(
  pixels: number,
  rangePixels: number,
  fine: boolean,
  fineFactor = 0.1
): number {
  if (rangePixels <= 0) {
    return 0
  }
  return (pixels / rangePixels) * (fine ? fineFactor : 1)
}

/** Parses typed text into a value inside the range, or null when unreadable. */
export function parseEntry(
  text: string,
  { min, max, step }: ValueRange,
  parse: (text: string) => number | null = parseNumber
): number | null {
  const parsed = parse(text)
  if (parsed === null || Number.isNaN(parsed)) {
    return null
  }
  return snapValue(parsed, min, max, step)
}

export function defaultFormat(
  value: number,
  { min, max, step }: ValueRange
): string {
  if (step && step > 0) {
    return value.toFixed(decimalsOf(step))
  }
  const span = max - min
  return value.toFixed(span >= 100 ? 0 : span >= 10 ? 1 : 2)
}

/** The props every value control in the kit shares. */
export type ValueControlProps = {
  value: number
  /** Called continuously while the value changes. Always inside a gesture. */
  onValueChange?: (value: number) => void
  /** Called before the first change of a drag, key hold, wheel burst or entry. */
  onGestureStart?: () => void
  /** Called when that drag, key hold, wheel burst or entry is over. */
  onGestureEnd?: () => void
  /** Restored by double-click and Ctrl/Cmd-click. */
  defaultValue?: number
  min?: number
  max?: number
  /** Snap values to `min + k * step`. Leave unset for a continuous control. */
  step?: number
  /**
   * With a `step`: what an arrow key or wheel notch moves by. Shift then
   * moves by one `step`. Defaults to the step.
   */
  keyStep?: number
  scale?: ValueScaleOption
  format?: (value: number) => string
  /** Turns typed text into a value. Must understand what `format` prints. */
  parse?: (text: string) => number | null
  /**
   * What the number is, such as `"gain"` or `"pan"`: see `ValueUnit`. The
   * control does nothing with it but hand it on in `ValueControlActions`,
   * for a menu that copies a value from one control to another and must
   * not paste a level into a pan.
   */
  unitKind?: string
  disabled?: boolean
}

/**
 * What something outside a value control can do with it, such as an entry
 * of a right-click menu: put it back to its default, open its text entry,
 * or give it a value read from text.
 */
export type ValueControlActions = {
  /** The value clamped to the range. */
  value: number
  /** The formatted readout. */
  text: string
  defaultValue: number | undefined
  disabled: boolean
  /** Whether Enter and typing open the text entry. */
  editable: boolean
  /** What the number is, when the control was told. */
  unitKind: string | undefined
  /** Restores the default as one gesture. Does nothing without one. */
  reset: () => void
  /** Opens the text entry, as Enter does. */
  startEditing: () => void
  /** Applies a value as a gesture of its own. */
  change: (value: number) => void
  /**
   * Reads text the way the text entry does and brings it into the range.
   * Null when it is not a value for this control.
   */
  parse: (text: string) => number | null
}

export type ValueControlSlotProps = {
  control: ValueControlActions
  /** The control's own element. It takes a ref and event handlers. */
  children: React.ReactElement<React.ComponentProps<"div">>
}

/**
 * A place to wrap every value control under it, without the controls
 * knowing in what. An app provides a component here to give all of them
 * the same right-click menu; the kit itself provides nothing.
 *
 *     <ValueControlSlot value={MyContextMenu}>…</ValueControlSlot>
 */
export const ValueControlSlot =
  React.createContext<React.ComponentType<ValueControlSlotProps> | null>(null)

/**
 * Hands a control's element to the slot around it, if there is one, and
 * returns what to render.
 */
export function useValueControlSlot(
  control: ValueControlActions,
  element: React.ReactElement<React.ComponentProps<"div">>
): React.ReactElement {
  const Slot = React.useContext(ValueControlSlot)
  return Slot
    ? React.createElement(Slot, { control, children: element })
    : element
}

export type UseDragValueOptions = ValueControlProps & {
  /** The drag axis. Up and right increase the value. */
  orientation?: "vertical" | "horizontal"
  /** Pointer travel in pixels for the full range, read when a drag starts. */
  dragRange?: number | ((element: HTMLElement) => number)
  /** Speed multiplier while Shift is held. */
  fineFactor?: number
  /**
   * Keep counting pointer travel past the ends of the range, so a control
   * that moves with the pointer (a fader cap) stays under it on the way back.
   */
  trackOvershoot?: boolean
  /** Values the drag sticks to when it passes them, such as a pan center. */
  detents?: readonly number[]
  doubleClick?: "reset" | "edit" | "none"
  wheel?: boolean
  /** Allow Enter and typing to open the text entry. */
  editable?: boolean
  /**
   * A value something else is giving the control right now. The slider
   * says it in `aria-valuetext`, a few times a second, so a screen reader
   * hears where the control is and not only where it was left.
   */
  live?: LiveValueFeed
}

type DragState = {
  pointerId: number
  x: number
  y: number
  position: number
  range: number
}

type TapState = { time: number; x: number; y: number }

const DEFAULT_DRAG_RANGE = 200
const DETENT_TRAVEL = 0.02
const WHEEL_IDLE_MS = 400
const SCROLL_GUARD_MS = 250
const DOUBLE_TAP_MS = 350
const DOUBLE_TAP_PIXELS = 24
/** A live value is said four times a second at most. */
export const LIVE_TEXT_MS = 250

/** What a slider says while something else is moving it. */
export function liveValueText(live: string, stored: string): string {
  return message("{live}, automated (set to {stored})", { live, stored })
}

// One passive listener for the whole page notes when a wheel event scrolled
// something else, so a control never grabs the wheel in the middle of a scroll.
let lastScrollTime = -Infinity
let scrollWatchers = 0

function noteScroll(event: WheelEvent) {
  if (!event.defaultPrevented) {
    lastScrollTime = event.timeStamp
  }
}

function watchScroll(): () => void {
  if (scrollWatchers === 0) {
    window.addEventListener("wheel", noteScroll, { passive: true })
  }
  scrollWatchers += 1
  return () => {
    scrollWatchers -= 1
    if (scrollWatchers === 0) {
      window.removeEventListener("wheel", noteScroll)
    }
  }
}

function wheelPixels(event: WheelEvent): number {
  // Browsers turn Shift+wheel into a sideways scroll.
  const delta = event.deltaY !== 0 ? event.deltaY : event.deltaX
  if (event.deltaMode === 1) {
    return delta * 40
  }
  if (event.deltaMode === 2) {
    return delta * 400
  }
  return delta
}

const STARTS_ENTRY = /^[0-9.,+\-−]$/

/**
 * The interaction model shared by every value control: vertical drag with
 * pointer capture, Shift for fine adjust, double-click or Ctrl/Cmd-click to
 * reset, wheel, keyboard stepping and typed entry.
 *
 * Delete and Backspace are not the control's keys. In an app they delete
 * things, and a focused knob that took them to reset itself made the same
 * key mean two things a click apart.
 */
export function useDragValue(options: UseDragValueOptions) {
  const {
    value: rawValue,
    min = 0,
    max = 1,
    step,
    keyStep,
    scale,
    format,
    disabled = false,
    orientation = "vertical",
    doubleClick = "reset",
    wheel = true,
    editable = true,
  } = options

  const range: ValueRange = { min, max, step, keyStep, scale }
  const value = clampValue(rawValue, min, max)
  const normalized = valueToNormalized(value, min, max, scale)
  const text = format ? format(value) : defaultFormat(value, range)

  const [node, setNode] = React.useState<HTMLElement | null>(null)
  const [dragging, setDragging] = React.useState(false)
  const [entry, setEntry] = React.useState<{ seed: string | null } | null>(null)
  const editing = entry !== null

  // Timers, the wheel listener and unmount cleanup run outside render and
  // need the props of the latest render.
  const latest = React.useRef({ ...options, min, max, value, editing })
  React.useLayoutEffect(() => {
    latest.current = { ...options, min, max, value, editing }
  })

  // The last value handed to the app. Moves ahead of `value` between a
  // change and the render that brings it back.
  const current = React.useRef(value)
  React.useLayoutEffect(() => {
    current.current = value
  })

  const drag = React.useRef<DragState | null>(null)
  const lastTap = React.useRef<TapState | null>(null)
  const holds = React.useRef(new Set<string>())
  const gestureOpen = React.useRef(false)
  const wheelRest = React.useRef(0)
  const wheelTimer = React.useRef<ReturnType<typeof setTimeout> | null>(null)
  const armed = React.useRef(false)

  function hold(source: string) {
    holds.current.add(source)
  }

  function release(source: string) {
    holds.current.delete(source)
    if (holds.current.size === 0 && gestureOpen.current) {
      gestureOpen.current = false
      latest.current.onGestureEnd?.()
    }
  }

  function emit(next: number) {
    const props = latest.current
    const snapped = snapValue(next, props.min, props.max, props.step)
    if (snapped === current.current) {
      return
    }
    if (!gestureOpen.current) {
      gestureOpen.current = true
      props.onGestureStart?.()
    }
    current.current = snapped
    props.onValueChange?.(snapped)
  }

  function change(next: number) {
    hold("single")
    emit(next)
    release("single")
  }

  function reset() {
    const target = latest.current.defaultValue
    if (target !== undefined && !latest.current.disabled) {
      change(target)
    }
  }

  function startEditing(seed: string | null = null) {
    if (editable && !disabled) {
      setEntry({ seed })
    }
  }

  function stopEditing(reason?: "escape" | "blur") {
    setEntry(null)
    // An entry that lost the focus leaves it where it went.
    if (reason !== "blur") {
      node?.focus({ preventScroll: true })
    }
  }

  function commitEntry(typed: string) {
    const props = latest.current
    const parsed = parseEntry(typed, props, props.parse)
    stopEditing()
    if (parsed !== null) {
      change(parsed)
    }
  }

  function endDrag(pointerId: number) {
    if (drag.current?.pointerId !== pointerId) {
      return
    }
    drag.current = null
    setDragging(false)
    release("drag")
  }

  function handlePointerDown(event: React.PointerEvent<HTMLElement>) {
    if (disabled || editing || event.button !== 0 || drag.current) {
      return
    }
    // Focus is left to the browser's own handling of the press, so a mouse
    // press does not show the keyboard focus ring.
    const element = event.currentTarget
    if (event.ctrlKey || event.metaKey) {
      reset()
      return
    }
    if (event.pointerType !== "mouse" && doubleClick !== "none") {
      // Touch and pen do not reliably produce dblclick on a control that
      // has touch-action: none.
      const tap = { time: event.timeStamp, x: event.clientX, y: event.clientY }
      const previous = lastTap.current
      lastTap.current = tap
      if (
        previous &&
        tap.time - previous.time < DOUBLE_TAP_MS &&
        Math.hypot(tap.x - previous.x, tap.y - previous.y) < DOUBLE_TAP_PIXELS
      ) {
        lastTap.current = null
        if (doubleClick === "reset") {
          reset()
        } else {
          startEditing()
        }
        return
      }
    }
    try {
      element.setPointerCapture(event.pointerId)
    } catch {
      // No capture (an already released pointer): the drag still works
      // while the pointer stays over the control.
    }
    const { dragRange = DEFAULT_DRAG_RANGE } = options
    drag.current = {
      pointerId: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      position: valueToNormalized(current.current, min, max, scale),
      range: typeof dragRange === "function" ? dragRange(element) : dragRange,
    }
    hold("drag")
    setDragging(true)
  }

  function handlePointerMove(event: React.PointerEvent<HTMLElement>) {
    const state = drag.current
    if (!state || state.pointerId !== event.pointerId) {
      return
    }
    const pixels = logicalDelta(
      orientation === "horizontal"
        ? event.clientX - state.x
        : state.y - event.clientY
    )
    state.x = event.clientX
    state.y = event.clientY
    if (pixels === 0) {
      return
    }
    state.position += dragTravel(
      pixels,
      state.range,
      event.shiftKey,
      options.fineFactor
    )
    if (!options.trackOvershoot) {
      state.position = clamp01(state.position)
    }
    const position = clamp01(state.position)
    if (!event.shiftKey) {
      const detent = options.detents?.find(
        (stop) =>
          Math.abs(valueToNormalized(stop, min, max, scale) - position) <
          DETENT_TRAVEL
      )
      if (detent !== undefined) {
        emit(detent)
        return
      }
    }
    emit(normalizedToValue(position, min, max, scale))
  }

  function handlePointerEnd(event: React.PointerEvent<HTMLElement>) {
    endDrag(event.pointerId)
  }

  function handleDoubleClick() {
    if (disabled) {
      return
    }
    if (doubleClick === "reset") {
      reset()
    } else if (doubleClick === "edit") {
      startEditing()
    }
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLElement>) {
    if (disabled || editing || event.target !== event.currentTarget) {
      return
    }
    if (event.altKey || event.ctrlKey || event.metaKey) {
      return
    }
    const mode: StepMode = event.shiftKey ? "fine" : "normal"
    let next: number
    switch (event.key) {
      case "ArrowUp":
      case "ArrowRight":
        next = stepValue(current.current, 1, mode, range)
        break
      case "ArrowDown":
      case "ArrowLeft":
        next = stepValue(current.current, -1, mode, range)
        break
      case "PageUp":
        next = stepValue(current.current, 1, "coarse", range)
        break
      case "PageDown":
        next = stepValue(current.current, -1, "coarse", range)
        break
      case "Home":
        next = min
        break
      case "End":
        next = max
        break
      case "Enter":
        if (editable) {
          event.preventDefault()
          startEditing()
        }
        return
      default:
        if (editable && STARTS_ENTRY.test(event.key)) {
          event.preventDefault()
          startEditing(event.key)
        }
        return
    }
    event.preventDefault()
    hold("key")
    emit(next)
  }

  function handleKeyUp() {
    release("key")
  }

  function handleBlur() {
    release("key")
  }

  const handleWheel = React.useEffectEvent((event: WheelEvent) => {
    const props = latest.current
    if (props.disabled || props.editing || event.ctrlKey) {
      return
    }
    // The wheel belongs to the page unless the control has focus or the
    // pointer was moved onto it on purpose and no scroll is under way.
    const focused = document.activeElement === event.currentTarget
    const scrolling = event.timeStamp - lastScrollTime < SCROLL_GUARD_MS
    if (!focused && (!armed.current || scrolling)) {
      return
    }
    const pixels = wheelPixels(event)
    if (pixels === 0) {
      return
    }
    event.preventDefault()
    wheelRest.current -= pixels / 100
    const mode: StepMode = event.shiftKey ? "fine" : "normal"
    let next: number
    if (props.step && props.step > 0) {
      const notches = Math.trunc(wheelRest.current)
      if (notches === 0) {
        return
      }
      wheelRest.current -= notches
      next = stepValue(current.current, notches, mode, props)
    } else {
      next = stepValue(current.current, wheelRest.current, mode, props)
      wheelRest.current = 0
    }
    hold("wheel")
    emit(next)
    if (wheelTimer.current !== null) {
      clearTimeout(wheelTimer.current)
    }
    wheelTimer.current = setTimeout(() => {
      wheelTimer.current = null
      wheelRest.current = 0
      release("wheel")
    }, WHEEL_IDLE_MS)
  })

  React.useEffect(() => {
    if (!node || !wheel) {
      return undefined
    }
    let enterX = 0
    let enterY = 0
    const onEnter = (event: PointerEvent) => {
      enterX = event.clientX
      enterY = event.clientY
      armed.current = false
    }
    const onMove = (event: PointerEvent) => {
      // A control that scrolls under a resting pointer gets move events
      // too, but at the position it entered at.
      if (event.clientX !== enterX || event.clientY !== enterY) {
        armed.current = true
      }
    }
    const onLeave = () => {
      armed.current = false
    }
    const onWheel = (event: WheelEvent) => handleWheel(event)
    const unwatch = watchScroll()
    node.addEventListener("pointerenter", onEnter)
    node.addEventListener("pointermove", onMove)
    node.addEventListener("pointerleave", onLeave)
    // React attaches wheel listeners as passive, which cannot stop the scroll.
    node.addEventListener("wheel", onWheel, { passive: false })
    return () => {
      unwatch()
      node.removeEventListener("pointerenter", onEnter)
      node.removeEventListener("pointermove", onMove)
      node.removeEventListener("pointerleave", onLeave)
      node.removeEventListener("wheel", onWheel)
    }
  }, [node, wheel])

  // The live value, for assistive technology. It is written to the element
  // and not rendered: the feed can speak 60 times a second, and four are
  // plenty for a readout that is read aloud.
  const storedText = React.useRef(text)
  React.useLayoutEffect(() => {
    storedText.current = text
  })
  const liveFeed = options.live
  React.useEffect(() => {
    if (!liveFeed || !node) {
      return undefined
    }
    let written = -Infinity
    let waiting: number | null = null
    let timer: ReturnType<typeof setTimeout> | null = null
    const say = (shown: number) => {
      const props = latest.current
      const clamped = clampValue(shown, props.min, props.max)
      written = performance.now()
      node.setAttribute(
        "aria-valuetext",
        liveValueText(
          props.format ? props.format(clamped) : defaultFormat(clamped, props),
          storedText.current
        )
      )
    }
    const rest = () => {
      if (timer !== null) {
        clearTimeout(timer)
      }
      timer = null
      waiting = null
      node.setAttribute("aria-valuetext", storedText.current)
    }
    const stop = liveFeed((next) => {
      if (next === null) {
        rest()
        return
      }
      const since = performance.now() - written
      if (since >= LIVE_TEXT_MS) {
        say(next)
        return
      }
      // Too soon: the newest value is said when its turn comes.
      waiting = next
      timer ??= setTimeout(() => {
        timer = null
        if (waiting !== null) {
          say(waiting)
          waiting = null
        }
      }, LIVE_TEXT_MS - since)
    })
    return () => {
      stop()
      rest()
    }
  }, [liveFeed, node])

  // A control removed in the middle of a gesture still closes it.
  React.useEffect(() => {
    const open = gestureOpen
    const timer = wheelTimer
    return () => {
      if (timer.current !== null) {
        clearTimeout(timer.current)
      }
      if (open.current) {
        open.current = false
        latest.current.onGestureEnd?.()
      }
    }
  }, [])

  const sliderProps = {
    ref: setNode,
    role: "slider",
    tabIndex: disabled ? -1 : 0,
    "aria-valuemin": min,
    "aria-valuemax": max,
    "aria-valuenow": value,
    "aria-valuetext": text,
    "aria-orientation": orientation,
    "aria-disabled": disabled || undefined,
    "data-dragging": dragging ? "" : undefined,
    "data-disabled": disabled ? "" : undefined,
    onPointerDown: handlePointerDown,
    onPointerMove: handlePointerMove,
    onPointerUp: handlePointerEnd,
    onPointerCancel: handlePointerEnd,
    onLostPointerCapture: handlePointerEnd,
    onDoubleClick: handleDoubleClick,
    onKeyDown: handleKeyDown,
    onKeyUp: handleKeyUp,
    onBlur: handleBlur,
  } satisfies React.ComponentProps<"div"> & {
    [attribute: `data-${string}`]: string | undefined
  }

  return {
    /** The value clamped to the range. */
    value,
    /** The value's position along the control's travel, 0 to 1. */
    normalized,
    /** The formatted readout. */
    text,
    dragging,
    editing,
    /** Spread onto the element that acts as the slider. */
    sliderProps,
    /** Spread onto a `ValueInput` rendered while `editing` is true. */
    entryProps: {
      initialText: entry?.seed ?? text,
      selectAll: entry?.seed == null,
      onCommit: commitEntry,
      onCancel: stopEditing,
    },
    startEditing,
    reset,
    /** Applies a value as a gesture of its own. */
    change,
    /** The same, for whatever a `ValueControlSlot` wraps the control in. */
    actions: {
      value,
      text,
      defaultValue: options.defaultValue,
      disabled,
      editable,
      unitKind: options.unitKind,
      reset,
      startEditing,
      change,
      parse: (typed: string) =>
        parseEntry(typed, latest.current, latest.current.parse),
    } satisfies ValueControlActions,
  }
}

export type DragValue = ReturnType<typeof useDragValue>

/**
 * A feed of the value something else is giving a control right now, as an
 * automation curve does while a song plays. The control subscribes on
 * mount: it hands over a listener and gets back a function that stops the
 * feed. The listener takes the value in the control's own unit, or null
 * when nothing is moving the control, and optionally the CSS color to show
 * it in.
 *
 * Values may arrive 60 times a second. The control draws them straight to
 * the page and never renders for one.
 */
export type LiveValueFeed = (
  listener: (value: number | null, color?: string) => void
) => () => void

/**
 * Subscribes a control to a live value. `draw` is called with each value,
 * outside React's rendering, and with null once when the feed ends.
 */
export function useLiveValue(
  feed: LiveValueFeed | undefined,
  draw: (value: number | null, color?: string) => void
) {
  const onValue = React.useEffectEvent(draw)
  React.useEffect(() => {
    if (!feed) {
      return undefined
    }
    const stop = feed((value, color) => onValue(value, color))
    return () => {
      stop()
      onValue(null)
    }
  }, [feed])
}
