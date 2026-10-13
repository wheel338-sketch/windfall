// SPDX-License-Identifier: MIT
import * as React from "react"

import { cn } from "@/lib/utils"
import { translatedText as t } from "@/lib/i18n"

const BLACK = [0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0]
const NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
// How far a black key sits off the line between its two white keys, in
// white-key widths, by pitch class.
const BLACK_OFFSET: Record<number, number> = {
  1: -0.1,
  3: 0.1,
  6: -0.13,
  8: 0,
  10: 0.13,
}
const BLACK_WIDTH = 0.6
// The share of a white key's length that black keys cover.
const BLACK_DEPTH = 0.62
const MIN_VELOCITY = 0.2

export function isBlackKey(key: number): boolean {
  return BLACK[((key % 12) + 12) % 12] === 1
}

/**
 * The name of a MIDI key. `middleCOctave` picks the octave numbering: 5 makes
 * key 60 "C5" (the FL convention), 4 makes it "C4".
 */
export function noteName(key: number, middleCOctave = 5): string {
  const octave = Math.floor(key / 12) + (middleCOctave - 5)
  return `${NAMES[((key % 12) + 12) % 12]}${octave}`
}

export type KeyShape = {
  key: number
  black: boolean
  /** Where the key starts and ends along the keyboard, 0 (low) to 1 (high). */
  start: number
  end: number
}

export type KeyboardLayout = "classic" | "uniform"

/**
 * Places the keys from `lowKey` to `highKey`. "classic" gives every white key
 * the same width, like a real keyboard. "uniform" gives every semitone the
 * same space, which lines the keys up with the rows of a piano roll.
 */
export function layoutKeys(
  lowKey: number,
  highKey: number,
  layout: KeyboardLayout
): KeyShape[] {
  const shapes: KeyShape[] = []
  if (layout === "uniform") {
    const row = 1 / (highKey - lowKey + 1)
    for (let key = lowKey; key <= highKey; key += 1) {
      const at = (key - lowKey) * row
      if (isBlackKey(key)) {
        shapes.push({ key, black: true, start: at, end: at + row })
        continue
      }
      // A white key reaches halfway into the black rows beside it.
      const below = key > lowKey && isBlackKey(key - 1) ? row / 2 : 0
      const above = key < highKey && isBlackKey(key + 1) ? row / 2 : 0
      shapes.push({
        key,
        black: false,
        start: at - below,
        end: at + row + above,
      })
    }
    return shapes
  }
  // A classic keyboard cannot end on a black key.
  const low = isBlackKey(lowKey) ? lowKey - 1 : lowKey
  const high = isBlackKey(highKey) ? highKey + 1 : highKey
  let whites = 0
  for (let key = low; key <= high; key += 1) {
    whites += isBlackKey(key) ? 0 : 1
  }
  const width = 1 / whites
  let index = 0
  for (let key = low; key <= high; key += 1) {
    if (isBlackKey(key)) {
      const center = (index + BLACK_OFFSET[((key % 12) + 12) % 12]) * width
      shapes.push({
        key,
        black: true,
        start: center - (BLACK_WIDTH * width) / 2,
        end: center + (BLACK_WIDTH * width) / 2,
      })
    } else {
      shapes.push({
        key,
        black: false,
        start: index * width,
        end: (index + 1) * width,
      })
      index += 1
    }
  }
  return shapes
}

/**
 * The key at a point. `along` runs 0 (low) to 1 (high) and `depth` runs from
 * the back of the keys (0) to their front edge (1). `velocity` grows towards
 * the front of the key.
 */
export function keyAtPoint(
  shapes: readonly KeyShape[],
  along: number,
  depth: number
): { key: number; velocity: number } | null {
  if (along < 0 || along > 1 || depth < 0 || depth > 1) {
    return null
  }
  const velocity = (share: number) =>
    MIN_VELOCITY + (1 - MIN_VELOCITY) * Math.min(1, Math.max(0, share))
  if (depth <= BLACK_DEPTH) {
    const black = shapes.find(
      (shape) => shape.black && along >= shape.start && along < shape.end
    )
    if (black) {
      return { key: black.key, velocity: velocity(depth / BLACK_DEPTH) }
    }
  }
  const white = shapes.find(
    (shape) => !shape.black && along >= shape.start && along <= shape.end
  )
  return white ? { key: white.key, velocity: velocity(depth) } : null
}

export type PianoKeyboardHandle = {
  /** Lights a key for a moment without a React render. */
  flash: (key: number, durationMs?: number) => void
  /** Lights or unlights a key without a React render, for engine playback. */
  setLit: (key: number, lit: boolean) => void
  /** Sends note-off for every key held by a pointer or the keyboard. */
  releaseAll: () => void
}

type PianoKeyboardProps = Omit<
  React.ComponentProps<"div">,
  "ref" | "children"
> & {
  ref?: React.Ref<PianoKeyboardHandle>
  /** The lowest and highest MIDI keys shown. */
  lowKey?: number
  highKey?: number
  orientation?: "horizontal" | "vertical"
  /** Defaults to "classic" when horizontal and "uniform" when vertical. */
  layout?: KeyboardLayout
  /** Velocity is 0 to 1, louder towards the front edge of a key. */
  onNoteOn?: (key: number, velocity: number) => void
  onNoteOff?: (key: number) => void
  /** Keys shown as held, such as the notes sounding now. */
  activeKeys?: ReadonlySet<number> | readonly number[]
  /** Print note names on the C keys. */
  showLabels?: boolean
  /** Octave number of key 60. 5 (the default) follows FL, 4 follows most others. */
  middleCOctave?: number
  /** Velocity of notes played with Enter. */
  keyboardVelocity?: number
  /** Highlight color as any CSS color. Defaults to `--wf-brand`. */
  color?: string
  disabled?: boolean
}

const KEYBOARD_SOURCE = "keyboard"

// A key is highlighted when the `activeKeys` prop holds it (aria-pressed),
// when a pointer or the keyboard holds it (data-held) or when the app lit it
// through the ref (data-lit). The three are tracked apart so none of them can
// clear another.
const keyClass = {
  white:
    "bg-[var(--wf-key-white,oklch(0.97_0_0))] text-[oklch(0.45_0_0)] aria-pressed:bg-(--key-white-on) aria-pressed:text-[oklch(0.2_0_0)] data-held:bg-(--key-white-on) data-held:text-[oklch(0.2_0_0)] data-lit:bg-(--key-white-on) data-lit:text-[oklch(0.2_0_0)]",
  black:
    "z-10 bg-[var(--wf-key-black,oklch(0.2_0_0))] aria-pressed:bg-(--key-black-on) data-held:bg-(--key-black-on) data-lit:bg-(--key-black-on)",
}

/**
 * A playable piano keyboard. Dragging across keys plays a glissando, and
 * every note-on is followed by a note-off, whatever ends the press.
 */
function PianoKeyboard({
  ref,
  className,
  style,
  lowKey = 48,
  highKey = 72,
  orientation = "horizontal",
  layout,
  onNoteOn,
  onNoteOff,
  activeKeys,
  showLabels = true,
  middleCOctave = 5,
  keyboardVelocity = 0.8,
  color,
  disabled = false,
  ...props
}: PianoKeyboardProps) {
  const horizontal = orientation === "horizontal"
  const resolvedLayout = layout ?? (horizontal ? "classic" : "uniform")
  const shapes = React.useMemo(
    () => layoutKeys(lowKey, highKey, resolvedLayout),
    [lowKey, highKey, resolvedLayout]
  )
  const active = React.useMemo(
    () => (activeKeys instanceof Set ? activeKeys : new Set(activeKeys ?? [])),
    [activeKeys]
  )

  const rootRef = React.useRef<HTMLDivElement>(null)
  // Which key each pointer (or the keyboard) holds, and how many holders
  // each key has, so two fingers on one key send one note.
  const holders = React.useRef(new Map<number | string, number>())
  const counts = React.useRef(new Map<number, number>())
  const flashes = React.useRef(new Map<number, ReturnType<typeof setTimeout>>())
  const callbacks = React.useRef({ onNoteOn, onNoteOff })
  React.useLayoutEffect(() => {
    callbacks.current = { onNoteOn, onNoteOff }
  })

  const firstKey = shapes[0]?.key ?? lowKey
  const lastKey = shapes[shapes.length - 1]?.key ?? highKey
  const [cursor, setCursor] = React.useState(60)
  const tabStop = Math.min(lastKey, Math.max(firstKey, cursor))

  function keyElement(key: number): HTMLElement | null {
    return rootRef.current?.querySelector(`[data-key="${key}"]`) ?? null
  }

  function press(source: number | string, key: number, velocity: number) {
    holders.current.set(source, key)
    const count = counts.current.get(key) ?? 0
    counts.current.set(key, count + 1)
    if (count === 0) {
      keyElement(key)?.setAttribute("data-held", "")
      callbacks.current.onNoteOn?.(key, velocity)
    }
  }

  function release(source: number | string) {
    const key = holders.current.get(source)
    if (key === undefined) {
      return
    }
    holders.current.delete(source)
    const count = (counts.current.get(key) ?? 1) - 1
    if (count > 0) {
      counts.current.set(key, count)
      return
    }
    counts.current.delete(key)
    keyElement(key)?.removeAttribute("data-held")
    callbacks.current.onNoteOff?.(key)
  }

  function releaseAll() {
    for (const source of [...holders.current.keys()]) {
      release(source)
    }
  }

  React.useImperativeHandle(ref, () => ({
    flash(key, durationMs = 150) {
      const pending = flashes.current.get(key)
      if (pending !== undefined) {
        clearTimeout(pending)
      }
      keyElement(key)?.setAttribute("data-lit", "")
      flashes.current.set(
        key,
        setTimeout(() => {
          flashes.current.delete(key)
          keyElement(key)?.removeAttribute("data-lit")
        }, durationMs)
      )
    },
    setLit(key, lit) {
      keyElement(key)?.toggleAttribute("data-lit", lit)
    },
    releaseAll,
  }))

  // A window that loses focus never delivers the pointer-up, and an
  // unmounted keyboard cannot receive it.
  React.useEffect(() => {
    const held = holders.current
    const noteCounts = counts.current
    const timers = flashes.current
    const stopAll = () => {
      for (const key of new Set(held.values())) {
        rootRef.current
          ?.querySelector(`[data-key="${key}"]`)
          ?.removeAttribute("data-held")
        callbacks.current.onNoteOff?.(key)
      }
      held.clear()
      noteCounts.clear()
    }
    window.addEventListener("blur", stopAll)
    return () => {
      window.removeEventListener("blur", stopAll)
      stopAll()
      for (const timer of timers.values()) {
        clearTimeout(timer)
      }
      timers.clear()
    }
  }, [])

  function hit(event: React.PointerEvent<HTMLDivElement>) {
    const rect = event.currentTarget.getBoundingClientRect()
    if (rect.width === 0 || rect.height === 0) {
      // No layout (a test environment): use the key element that was hit.
      const key = (event.target as HTMLElement).closest<HTMLElement>(
        "[data-key]"
      )?.dataset.key
      return key === undefined
        ? null
        : { key: Number(key), velocity: keyboardVelocity }
    }
    const across = (event.clientX - rect.left) / rect.width
    const down = (event.clientY - rect.top) / rect.height
    return horizontal
      ? keyAtPoint(shapes, across, down)
      : keyAtPoint(shapes, 1 - down, across)
  }

  function handlePointerDown(event: React.PointerEvent<HTMLDivElement>) {
    if (disabled || event.button !== 0) {
      return
    }
    const target = hit(event)
    if (!target) {
      return
    }
    try {
      event.currentTarget.setPointerCapture(event.pointerId)
    } catch {
      // Without capture the note still ends when the pointer leaves.
    }
    setCursor(target.key)
    press(event.pointerId, target.key, target.velocity)
  }

  function handlePointerMove(event: React.PointerEvent<HTMLDivElement>) {
    const held = holders.current.get(event.pointerId)
    // A captured pointer with nothing held slid off the keys and may come back.
    const tracking =
      held !== undefined ||
      (event.buttons !== 0 &&
        event.currentTarget.hasPointerCapture?.(event.pointerId))
    if (!tracking) {
      return
    }
    const target = hit(event)
    if (target?.key === held) {
      return
    }
    release(event.pointerId)
    if (target && !disabled) {
      press(event.pointerId, target.key, target.velocity)
    }
  }

  function handlePointerEnd(event: React.PointerEvent<HTMLDivElement>) {
    release(event.pointerId)
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    const from = (event.target as HTMLElement).dataset.key
    if (from === undefined || event.altKey || event.ctrlKey || event.metaKey) {
      return
    }
    const key = Number(from)
    const moveTo = (next: number) => {
      const clamped = Math.min(lastKey, Math.max(firstKey, next))
      setCursor(clamped)
      keyElement(clamped)?.focus({ preventScroll: true })
    }
    switch (event.key) {
      case "Enter":
        if (!event.repeat && !disabled) {
          release(KEYBOARD_SOURCE)
          press(KEYBOARD_SOURCE, key, keyboardVelocity)
        }
        break
      case "ArrowRight":
      case "ArrowUp":
        moveTo(key + 1)
        break
      case "ArrowLeft":
      case "ArrowDown":
        moveTo(key - 1)
        break
      case "PageUp":
        moveTo(key + 12)
        break
      case "PageDown":
        moveTo(key - 12)
        break
      case "Home":
        moveTo(firstKey)
        break
      case "End":
        moveTo(lastKey)
        break
      default:
        return
    }
    event.preventDefault()
  }

  function handleKeyUp(event: React.KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Enter") {
      release(KEYBOARD_SOURCE)
    }
  }

  return (
    <div
      ref={rootRef}
      role="group"
      aria-label={t("Piano keyboard")}
      data-slot="piano-keyboard"
      data-orientation={orientation}
      data-disabled={disabled ? "" : undefined}
      className={cn(
        "relative touch-none overflow-hidden rounded-[3px] bg-[var(--wf-key-black,oklch(0.2_0_0))] shadow-[0_0_0_1px_var(--wf-grid-line-strong)] select-none data-disabled:opacity-50",
        horizontal ? "h-20 w-full" : "h-full w-16",
        className
      )}
      style={
        {
          "--key-active": color ?? "var(--wf-key-active, var(--wf-brand))",
          "--key-white-on": "color-mix(in oklch, var(--key-active) 65%, white)",
          "--key-black-on": "color-mix(in oklch, var(--key-active) 80%, black)",
          ...style,
        } as React.CSSProperties
      }
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerEnd}
      onPointerCancel={handlePointerEnd}
      onLostPointerCapture={handlePointerEnd}
      onKeyDown={handleKeyDown}
      onKeyUp={handleKeyUp}
      onBlur={() => release(KEYBOARD_SOURCE)}
      onFocus={(event) => {
        const key = (event.target as HTMLElement).dataset.key
        if (key !== undefined) {
          setCursor(Number(key))
        }
      }}
      onContextMenu={(event) => event.preventDefault()}
      {...props}
    >
      {shapes.map((shape) => {
        const size = `${((shape.end - shape.start) * 100).toFixed(4)}%`
        const offset = `${(shape.start * 100).toFixed(4)}%`
        const pressed = active.has(shape.key)
        return (
          <div
            key={shape.key}
            role="button"
            tabIndex={!disabled && shape.key === tabStop ? 0 : -1}
            aria-label={noteName(shape.key, middleCOctave)}
            aria-pressed={pressed}
            aria-disabled={disabled || undefined}
            data-key={shape.key}
            className={cn(
              "absolute outline-none focus-visible:z-20 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
              shape.black ? keyClass.black : keyClass.white,
              horizontal
                ? shape.black
                  ? "top-0 rounded-b-[2px]"
                  : "inset-y-0 rounded-b-[2px] shadow-[inset_-1px_0_0_var(--wf-key-line,oklch(0.55_0_0))]"
                : shape.black
                  ? "left-0 rounded-r-[2px]"
                  : "inset-x-0 shadow-[inset_0_1px_0_var(--wf-key-line,oklch(0.55_0_0))]"
            )}
            style={
              horizontal
                ? {
                    left: offset,
                    width: size,
                    height: shape.black ? `${BLACK_DEPTH * 100}%` : undefined,
                  }
                : {
                    bottom: offset,
                    height: size,
                    width: shape.black ? `${BLACK_DEPTH * 100}%` : undefined,
                  }
            }
          >
            {showLabels && shape.key % 12 === 0 ? (
              <span
                aria-hidden="true"
                className={cn(
                  "pointer-events-none absolute text-[9px] leading-none font-medium",
                  horizontal
                    ? "inset-x-0 bottom-1 text-center"
                    : "right-1 bottom-0.5"
                )}
              >
                {noteName(shape.key, middleCOctave)}
              </span>
            ) : null}
          </div>
        )
      })}
    </div>
  )
}

export { PianoKeyboard }
export type { PianoKeyboardProps }
