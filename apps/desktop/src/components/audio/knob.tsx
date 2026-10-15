// SPDX-License-Identifier: MIT
import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

import {
  clampValue,
  defaultFormat,
  useDragValue,
  useLiveValue,
  useValueControlSlot,
  valueToNormalized,
  type LiveValueFeed,
  type ValueControlProps,
} from "./use-drag-value"
import { ValueInput } from "./value-input"
import { translatedText as t } from "@/lib/i18n"

const knobVariants = cva(
  "group/knob relative inline-flex flex-col items-center select-none data-disabled:opacity-50",
  {
    variants: {
      size: {
        sm: "gap-0.5 text-[10px]",
        md: "gap-1 text-[10px]",
        lg: "gap-1 text-[11px]",
      },
    },
    defaultVariants: { size: "md" },
  }
)

// Drawn at its pixel size so strokes land the same way at every scale.
const GEOMETRY = {
  sm: { size: 24, stroke: 2.5, gap: 1.5, pointer: 1.5 },
  md: { size: 36, stroke: 3, gap: 2.5, pointer: 2 },
  lg: { size: 52, stroke: 4, gap: 3, pointer: 2.5 },
} as const

const SWEEP_START = -135
const SWEEP = 270
// Half the width, in degrees, of the notch that marks a bipolar center.
const NOTCH = 5

function pointAt(center: number, radius: number, degrees: number) {
  const radians = (degrees * Math.PI) / 180
  return {
    x: center + radius * Math.sin(radians),
    y: center - radius * Math.cos(radians),
  }
}

function arcPath(center: number, radius: number, from: number, to: number) {
  const start = pointAt(center, radius, from)
  const end = pointAt(center, radius, to)
  const large = to - from > 180 ? 1 : 0
  return `M ${start.x.toFixed(3)} ${start.y.toFixed(3)} A ${radius} ${radius} 0 ${large} 1 ${end.x.toFixed(3)} ${end.y.toFixed(3)}`
}

type KnobProps = Omit<
  React.ComponentProps<"div">,
  "defaultValue" | "onChange"
> &
  ValueControlProps &
  VariantProps<typeof knobVariants> & {
    /** Draw the arc from the center value outwards, as for pan. */
    bipolar?: boolean
    /** The value a bipolar arc grows from. Defaults to the middle of the range. */
    center?: number
    /** Shown under the knob. While dragging it shows the value instead. */
    label?: React.ReactNode
    /** Always show the value under the knob. */
    showValue?: boolean
    /** Arc color as any CSS color. Defaults to `--wf-control-fill`. */
    color?: string
    /** Pointer travel in pixels for the full range. */
    dragRange?: number
    /**
     * A value something else is moving the knob to, such as an automation
     * curve while a song plays. The knob shows it with a second pointer and
     * arc, drawn outside React, and dims its own. `value` stays what a drag
     * changes.
     */
    live?: LiveValueFeed
    /**
     * A small dot at the knob's corner in this CSS color: something else
     * can move this value. It is also the color of the live pointer when
     * the feed gives none.
     */
    marker?: string
  }

function Knob({
  className,
  style,
  size = "md",
  bipolar = false,
  center,
  label,
  showValue = false,
  color,
  dragRange,
  live,
  marker,
  value,
  onValueChange,
  onGestureStart,
  onGestureEnd,
  defaultValue,
  min = 0,
  max = 1,
  step,
  keyStep,
  scale,
  format,
  parse,
  unitKind,
  disabled = false,
  "aria-label": ariaLabel,
  "aria-labelledby": ariaLabelledBy,
  ...props
}: KnobProps) {
  const origin = center ?? (min + max) / 2
  const control = useDragValue({
    value,
    onValueChange,
    onGestureStart,
    onGestureEnd,
    defaultValue,
    min,
    max,
    step,
    keyStep,
    scale,
    format,
    parse,
    unitKind,
    disabled,
    dragRange,
    live,
    detents: bipolar ? [origin] : undefined,
  })
  const labelId = React.useId()
  const liveGroup = React.useRef<SVGGElement>(null)
  const liveArc = React.useRef<SVGPathElement>(null)
  const liveNeedle = React.useRef<SVGGElement>(null)
  const liveText = React.useRef<HTMLSpanElement>(null)

  const shape = GEOMETRY[size ?? "md"]
  const middle = shape.size / 2
  const radius = middle - shape.stroke / 2 - 0.5
  const capRadius = radius - shape.stroke / 2 - shape.gap
  const angle = SWEEP_START + control.normalized * SWEEP
  const originAngle =
    SWEEP_START + valueToNormalized(origin, min, max, scale) * SWEEP

  let fill: string | null = null
  if (bipolar) {
    if (Math.abs(angle - originAngle) > 0.5) {
      fill = arcPath(
        middle,
        radius,
        Math.min(angle, originAngle),
        Math.max(angle, originAngle)
      )
    }
  } else if (control.normalized > 0.002) {
    fill = arcPath(middle, radius, SWEEP_START, angle)
  }

  const track = bipolar
    ? [
        arcPath(middle, radius, SWEEP_START, originAngle - NOTCH),
        arcPath(middle, radius, originAngle + NOTCH, SWEEP_START + SWEEP),
      ].join(" ")
    : arcPath(middle, radius, SWEEP_START, SWEEP_START + SWEEP)

  const pointerFrom = pointAt(middle, capRadius * 0.3, angle)
  const pointerTo = pointAt(middle, capRadius * 0.82, angle)
  const liveLabel = !showValue && control.dragging

  useLiveValue(live, (next, liveColor) => {
    const group = liveGroup.current
    if (!group) {
      return
    }
    // The root, whatever slot name a control built on the knob gave it.
    const root = group.closest<HTMLElement>("[data-size]")
    if (next === null) {
      group.style.display = "none"
      root?.removeAttribute("data-live")
      return
    }
    const shown = clampValue(next, min, max)
    const at = valueToNormalized(shown, min, max, scale)
    const from = valueToNormalized(origin, min, max, scale)
    group.style.display = ""
    group.style.setProperty(
      "--live-color",
      liveColor ?? marker ?? "var(--wf-playhead)"
    )
    root?.setAttribute("data-live", "")
    // The arc is one path a dash is moved along, so no path is rebuilt.
    if (liveArc.current) {
      liveArc.current.style.strokeDasharray = bipolar
        ? `0 ${Math.min(at, from)} ${Math.abs(at - from)} 1`
        : `${at} 1`
    }
    liveNeedle.current?.setAttribute(
      "transform",
      `rotate(${(SWEEP_START + at * SWEEP).toFixed(2)} ${middle} ${middle})`
    )
    if (liveText.current) {
      liveText.current.textContent = format
        ? format(shown)
        : defaultFormat(shown, { min, max, step })
    }
  })

  return useValueControlSlot(
    control.actions,
    <div
      data-slot="knob"
      data-size={size}
      data-bipolar={bipolar ? "" : undefined}
      data-disabled={disabled ? "" : undefined}
      className={cn(knobVariants({ size }), className)}
      style={
        color
          ? ({ "--knob-fill": color, ...style } as React.CSSProperties)
          : style
      }
      {...props}
    >
      <div
        {...control.sliderProps}
        data-slot="knob-control"
        aria-label={ariaLabel}
        aria-labelledby={
          ariaLabelledBy ?? (label && !ariaLabel ? labelId : undefined)
        }
        title={label || showValue ? undefined : control.text}
        className="relative cursor-ns-resize touch-none rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background data-disabled:cursor-default"
      >
        {marker ? (
          <span
            data-slot="knob-marker"
            aria-hidden="true"
            className="pointer-events-none absolute top-0 right-0 size-[5px] rounded-full ring-1 ring-background"
            style={{ backgroundColor: marker }}
          />
        ) : null}
        <svg
          width={shape.size}
          height={shape.size}
          viewBox={`0 0 ${shape.size} ${shape.size}`}
          fill="none"
          aria-hidden="true"
          className="block"
        >
          <path
            d={track}
            stroke="var(--wf-control-track)"
            strokeWidth={shape.stroke}
            strokeLinecap="round"
          />
          {fill ? (
            <path
              d={fill}
              stroke="var(--knob-fill, var(--wf-control-fill))"
              strokeWidth={shape.stroke}
              strokeLinecap="round"
              className="group-data-live/knob:opacity-25"
            />
          ) : null}
          <circle
            cx={middle}
            cy={middle}
            r={capRadius}
            className="fill-[var(--wf-knob-cap,color-mix(in_oklch,var(--foreground)_14%,var(--background)))] stroke-(--wf-grid-line-strong) transition-[fill] group-hover/knob:fill-[var(--wf-knob-cap-hover,color-mix(in_oklch,var(--foreground)_22%,var(--background)))]"
            strokeWidth={1}
          />
          <line
            x1={pointerFrom.x}
            y1={pointerFrom.y}
            x2={pointerTo.x}
            y2={pointerTo.y}
            stroke="var(--foreground)"
            strokeWidth={shape.pointer}
            strokeLinecap="round"
            className="group-data-live/knob:opacity-25"
          />
          {live ? (
            <g
              ref={liveGroup}
              data-slot="knob-live"
              style={{ display: "none" }}
            >
              <path
                ref={liveArc}
                d={arcPath(middle, radius, SWEEP_START, SWEEP_START + SWEEP)}
                pathLength={1}
                stroke="var(--live-color)"
                strokeWidth={shape.stroke}
                strokeDasharray="0 1"
              />
              <g ref={liveNeedle}>
                <line
                  x1={middle}
                  y1={middle - capRadius * 0.3}
                  x2={middle}
                  y2={middle - capRadius * 0.82}
                  stroke="var(--live-color)"
                  strokeWidth={shape.pointer}
                  strokeLinecap="round"
                />
                <circle
                  cx={middle}
                  cy={middle - radius}
                  r={shape.stroke * 0.8}
                  fill="var(--live-color)"
                  stroke="var(--background)"
                  strokeWidth={1}
                />
              </g>
            </g>
          ) : null}
        </svg>
      </div>
      {label ? (
        <span
          id={labelId}
          data-slot="knob-label"
          // A long label is cut short to the knob's width; this keeps it whole.
          title={typeof label === "string" ? label : undefined}
          className="relative max-w-16 leading-none text-muted-foreground"
        >
          <span
            // A line as tall as its letters cuts the tails off g, p and q.
            // The padding gives them room, and the margin takes it back.
            className={cn(
              "-my-[0.25em] block truncate py-[0.25em]",
              liveLabel && "invisible"
            )}
          >
            {label}
          </span>
          {liveLabel ? (
            // Laid over the label so a long value does not push neighbors.
            <span
              aria-hidden="true"
              className="absolute top-0 left-1/2 -translate-x-1/2 whitespace-nowrap text-foreground tabular-nums"
            >
              {control.text}
            </span>
          ) : null}
        </span>
      ) : null}
      {showValue ? (
        <span
          data-slot="knob-value"
          className="leading-none text-foreground tabular-nums"
          onDoubleClick={() => control.startEditing()}
        >
          {live ? (
            <>
              <span className="group-data-live/knob:hidden">
                {control.text}
              </span>
              <span
                ref={liveText}
                data-slot="knob-live-value"
                className="hidden group-data-live/knob:inline"
              />
            </>
          ) : (
            control.text
          )}
        </span>
      ) : null}
      {control.editing ? (
        <ValueInput
          {...control.entryProps}
          aria-label={
            ariaLabel ?? (typeof label === "string" ? label : t("Value"))
          }
          className="absolute -bottom-[3px] left-1/2 z-10 h-4 w-14 -translate-x-1/2"
        />
      ) : null}
    </div>
  )
}

export { Knob, knobVariants }
export type { KnobProps }
