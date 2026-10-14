// SPDX-License-Identifier: MIT
import * as React from "react"

import { cn } from "@/lib/utils"

import { StepButton } from "./step-button"

export type StepGridHandle = {
  /** Moves the playhead highlight without a React render. */
  setPlayStep: (step: number | null) => void
}

type PaintState = {
  pointerId: number
  on: boolean
  last: number
  left: number
  width: number
  painted: Set<number>
}

const ASKED_FOR_MS = 250

function markPlaying(
  previous: Element | null,
  next: Element | null
): Element | null {
  if (previous !== next) {
    previous?.removeAttribute("data-playing")
    next?.setAttribute("data-playing", "")
  }
  return next
}

type StepGridProps = Omit<
  React.ComponentProps<"div">,
  "ref" | "onToggle" | "children"
> & {
  ref?: React.Ref<StepGridHandle>
  /** One entry per step. The length sets the number of steps. */
  steps: readonly boolean[]
  onToggle?: (step: number, on: boolean) => void
  /** A paint stroke or a key press that changes steps starts a gesture. */
  onGestureStart?: () => void
  onGestureEnd?: () => void
  /** The lit color as any CSS color. Defaults to `--wf-step-on`. */
  color?: string
  /** Steps per beat. Beats alternate between the two unlit shades. */
  groupSize?: number
  /** Optional per-step shade from a mixed musical meter map. */
  beatShades?: readonly boolean[]
  size?: "sm" | "md" | "lg"
  disabled?: boolean
  /** Right-click and right-drag clear steps. Turn off to allow a context menu. */
  rightClickClears?: boolean
  /**
   * Let Space toggle the focused step as well as Enter. Off by default, so
   * an app can keep Space for its transport.
   */
  spaceToggles?: boolean
  /** Names a step for screen readers. Defaults to "Step 1", "Step 2"... */
  stepLabel?: (step: number) => string
}

const defaultStepLabel = (step: number) => `Step ${step + 1}`

/**
 * One row of a step sequencer. Click toggles a step; dragging paints the
 * opposite of the first step's state across the row; right-drag clears.
 *
 * Each step is a single memoized button and all pointer handling sits on the
 * row, so a toggle re-renders one button and the playhead is one attribute
 * write. That keeps thousands of steps cheap while every step stays a real
 * button for the keyboard and for screen readers.
 */
const StepGrid = React.memo(function StepGrid({
  ref,
  className,
  style,
  steps,
  onToggle,
  onGestureStart,
  onGestureEnd,
  color,
  groupSize = 4,
  beatShades,
  size = "md",
  disabled = false,
  rightClickClears = true,
  spaceToggles = false,
  stepLabel = defaultStepLabel,
  ...props
}: StepGridProps) {
  const rootRef = React.useRef<HTMLDivElement>(null)
  const paint = React.useRef<PaintState | null>(null)
  const playing = React.useRef<Element | null>(null)
  const gestureOpen = React.useRef(false)
  const asked = React.useRef(new Map<number, { on: boolean; time: number }>())
  React.useLayoutEffect(() => {
    asked.current.clear()
  }, [steps])
  const [focusStep, setFocusStep] = React.useState(0)
  const tabStop = Math.min(focusStep, steps.length - 1)

  React.useImperativeHandle(
    ref,
    () => ({
      setPlayStep(step) {
        const next =
          step === null ? null : (rootRef.current?.children[step] ?? null)
        playing.current = markPlaying(playing.current, next)
      },
    }),
    []
  )

  // Input can outrun React: a press may land before the render that carries
  // the last stroke's changes. Recent changes are remembered for a moment so
  // the row decides from what it already asked for.
  function isOn(step: number): boolean {
    const recent = asked.current.get(step)
    if (recent && performance.now() - recent.time < ASKED_FOR_MS) {
      return recent.on
    }
    return steps[step]
  }

  function toggle(step: number, on: boolean) {
    if (isOn(step) === on) {
      return
    }
    asked.current.set(step, { on, time: performance.now() })
    if (!gestureOpen.current) {
      gestureOpen.current = true
      onGestureStart?.()
    }
    onToggle?.(step, on)
  }

  function endGesture() {
    if (gestureOpen.current) {
      gestureOpen.current = false
      onGestureEnd?.()
    }
  }

  function stepAt(state: Pick<PaintState, "left" | "width">, clientX: number) {
    const step = Math.floor(
      ((clientX - state.left) / state.width) * steps.length
    )
    return Math.min(steps.length - 1, Math.max(0, step))
  }

  function focusOn(step: number) {
    const target = rootRef.current?.children[step]
    if (target instanceof HTMLElement) {
      setFocusStep(step)
      target.focus({ preventScroll: true })
    }
  }

  function handlePointerDown(event: React.PointerEvent<HTMLDivElement>) {
    const clearing = event.button === 2
    if (disabled || paint.current || steps.length === 0) {
      return
    }
    if (clearing ? !rightClickClears : event.button !== 0) {
      return
    }
    const rect = event.currentTarget.getBoundingClientRect()
    const step =
      rect.width > 0
        ? stepAt(rect, event.clientX)
        : // No layout (a test environment): fall back to the step hit.
          Number(
            (event.target as HTMLElement).closest<HTMLElement>("[data-step]")
              ?.dataset.step ?? 0
          )
    try {
      event.currentTarget.setPointerCapture(event.pointerId)
    } catch {
      // Without capture the stroke still paints while over the row.
    }
    paint.current = {
      pointerId: event.pointerId,
      on: clearing ? false : !isOn(step),
      last: step,
      left: rect.left,
      width: rect.width,
      painted: new Set([step]),
    }
    setFocusStep(step)
    toggle(step, paint.current.on)
  }

  function handlePointerMove(event: React.PointerEvent<HTMLDivElement>) {
    const state = paint.current
    if (!state || state.pointerId !== event.pointerId || state.width <= 0) {
      return
    }
    const step = stepAt(state, event.clientX)
    if (step === state.last) {
      return
    }
    // A fast drag skips steps; fill in everything it passed over.
    const direction = step > state.last ? 1 : -1
    for (
      let at = state.last + direction;
      at !== step + direction;
      at += direction
    ) {
      if (!state.painted.has(at)) {
        state.painted.add(at)
        toggle(at, state.on)
      }
    }
    state.last = step
  }

  function handlePointerEnd(event: React.PointerEvent<HTMLDivElement>) {
    if (paint.current?.pointerId === event.pointerId) {
      paint.current = null
      endGesture()
    }
  }

  function handleClick(event: React.MouseEvent<HTMLDivElement>) {
    // Pointer presses were handled on the way down. A click with no detail
    // comes from Enter, from Space where that is allowed, or from assistive
    // technology.
    const step = (event.target as HTMLElement).closest<HTMLElement>(
      "[data-step]"
    )?.dataset.step
    if (disabled || event.detail !== 0 || step === undefined) {
      return
    }
    toggle(Number(step), !isOn(Number(step)))
    endGesture()
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    const from = (event.target as HTMLElement).dataset.step
    if (from === undefined || event.altKey || event.ctrlKey || event.metaKey) {
      return
    }
    const step = Number(from)
    switch (event.key) {
      case "ArrowLeft":
        focusOn(Math.max(0, step - 1))
        break
      case "ArrowRight":
        focusOn(Math.min(steps.length - 1, step + 1))
        break
      case "Home":
        focusOn(0)
        break
      case "End":
        focusOn(steps.length - 1)
        break
      case "ArrowUp":
      case "ArrowDown": {
        // Rows inside a StepGridGroup form one grid for the arrow keys.
        const root = rootRef.current
        const rows = root
          ?.closest('[data-slot="step-grid-group"]')
          ?.querySelectorAll('[data-slot="step-grid"]')
        if (!root || !rows) {
          return
        }
        const index = Array.prototype.indexOf.call(rows, root)
        const row = rows[index + (event.key === "ArrowUp" ? -1 : 1)]
        const target = row?.children[Math.min(step, row.children.length - 1)]
        if (!(target instanceof HTMLElement)) {
          return
        }
        target.focus({ preventScroll: true })
        break
      }
      default:
        return
    }
    event.preventDefault()
  }

  return (
    <div
      ref={rootRef}
      role="group"
      data-slot="step-grid"
      data-disabled={disabled ? "" : undefined}
      className={cn(
        "grid w-full touch-none gap-0.5 select-none",
        size === "sm" ? "h-4" : size === "lg" ? "h-8" : "h-6",
        className
      )}
      style={
        {
          gridTemplateColumns: `repeat(${steps.length}, minmax(0, 1fr))`,
          ...(color ? { "--step-on": color } : null),
          ...style,
        } as React.CSSProperties
      }
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerEnd}
      onPointerCancel={handlePointerEnd}
      onLostPointerCapture={handlePointerEnd}
      onClick={handleClick}
      onKeyDown={handleKeyDown}
      onFocus={(event) => {
        const step = (event.target as HTMLElement).dataset.step
        if (step !== undefined) {
          setFocusStep(Number(step))
        }
      }}
      onContextMenu={(event) => {
        if (rightClickClears) {
          event.preventDefault()
        }
      }}
      {...props}
    >
      {steps.map((on, step) => (
        <StepButton
          key={step}
          data-step={step}
          on={on}
          alt={beatShades?.[step] ?? Math.floor(step / groupSize) % 2 === 1}
          tabIndex={step === tabStop ? 0 : -1}
          disabled={disabled}
          spaceToggles={spaceToggles}
          aria-label={stepLabel(step)}
          className="size-full"
        />
      ))}
    </div>
  )
})

export type StepGridGroupHandle = {
  /** Moves the playhead highlight of every row in the group. */
  setPlayStep: (step: number | null) => void
}

type StepGridGroupProps = Omit<React.ComponentProps<"div">, "ref"> & {
  ref?: React.Ref<StepGridGroupHandle>
}

/**
 * Wraps the rows of a channel rack. Arrow up and down move between its rows,
 * and one `setPlayStep` call moves the playhead of all of them.
 */
function StepGridGroup({ ref, className, ...props }: StepGridGroupProps) {
  const rootRef = React.useRef<HTMLDivElement>(null)
  const playing = React.useRef<Element[]>([])

  React.useImperativeHandle(
    ref,
    () => ({
      setPlayStep(step) {
        for (const element of playing.current) {
          element.removeAttribute("data-playing")
        }
        playing.current = []
        if (step === null || !rootRef.current) {
          return
        }
        const rows = rootRef.current.querySelectorAll('[data-slot="step-grid"]')
        for (const row of rows) {
          const element = row.children[step]
          if (element) {
            element.setAttribute("data-playing", "")
            playing.current.push(element)
          }
        }
      },
    }),
    []
  )

  return (
    <div
      ref={rootRef}
      data-slot="step-grid-group"
      className={cn("flex flex-col gap-1", className)}
      {...props}
    />
  )
}

export { StepGrid, StepGridGroup }
export type { StepGridProps, StepGridGroupProps }
