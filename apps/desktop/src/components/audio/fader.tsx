// SPDX-License-Identifier: MIT
import * as React from "react"
import { cva } from "class-variance-authority"

import { cn } from "@/lib/utils"

import { dbToGain, formatGain, gainToDb, parseGain } from "./units"
import {
  clampValue,
  defaultFormat,
  useDragValue,
  useLiveValue,
  useValueControlSlot,
  valueToNormalized,
  type LiveValueFeed,
  type ValueControlProps,
  type ValueScale,
} from "./use-drag-value"
import { ValueInput } from "./value-input"
import { translatedText as t } from "@/lib/i18n"

/** The top of the fader: linear gain 2.0, about +6 dB. */
export const FADER_MAX_GAIN = 2

// The taper is straight lines between these points on a dB scale, steeper
// towards the top so the useful range around 0 dB gets most of the travel.
// Below the last point the gain falls in a straight line to silence.
const TAPER: readonly (readonly [db: number, position: number])[] = [
  [gainToDb(FADER_MAX_GAIN), 1],
  [0, 0.78],
  [-6, 0.62],
  [-12, 0.48],
  [-24, 0.27],
  [-48, 0.07],
]
const [FLOOR_DB, FLOOR_POSITION] = TAPER[TAPER.length - 1]
const FLOOR_GAIN = dbToGain(FLOOR_DB)

/**
 * Linear gain (0 to 2.0) to fader travel (0 to 1). Silence is 0, 0 dB is
 * 0.78 and +6 dB is 1.
 */
export function gainToFaderPosition(gain: number): number {
  if (!(gain > 0)) {
    return 0
  }
  if (gain >= FADER_MAX_GAIN) {
    return 1
  }
  if (gain <= FLOOR_GAIN) {
    return (gain / FLOOR_GAIN) * FLOOR_POSITION
  }
  const db = gainToDb(gain)
  for (let index = 1; index < TAPER.length; index += 1) {
    const [lowDb, lowPosition] = TAPER[index]
    if (db >= lowDb) {
      const [highDb, highPosition] = TAPER[index - 1]
      const share = (db - lowDb) / (highDb - lowDb)
      return lowPosition + share * (highPosition - lowPosition)
    }
  }
  return FLOOR_POSITION
}

/** Fader travel (0 to 1) to linear gain (0 to 2.0). */
export function faderPositionToGain(position: number): number {
  if (!(position > 0)) {
    return 0
  }
  if (position >= 1) {
    return FADER_MAX_GAIN
  }
  if (position <= FLOOR_POSITION) {
    return (position / FLOOR_POSITION) * FLOOR_GAIN
  }
  for (let index = 1; index < TAPER.length; index += 1) {
    const [lowDb, lowPosition] = TAPER[index]
    if (position >= lowPosition) {
      const [highDb, highPosition] = TAPER[index - 1]
      const share = (position - lowPosition) / (highPosition - lowPosition)
      return dbToGain(lowDb + share * (highDb - lowDb))
    }
  }
  return FLOOR_GAIN
}

/** The dB taper as a control scale. It assumes a range of 0 to 2.0. */
export const faderTaper: ValueScale = {
  toNormalized: (value) => gainToFaderPosition(value),
  fromNormalized: (normalized) => faderPositionToGain(normalized),
}

export type FaderTick = {
  /** Where the tick sits, in the fader's own value (linear gain by default). */
  value: number
  label?: string
  /** Draw the tick stronger, as for 0 dB. */
  strong?: boolean
}

export const FADER_DB_TICKS: readonly FaderTick[] = [
  { value: FADER_MAX_GAIN, label: "+6" },
  { value: 1, label: "0", strong: true },
  { value: dbToGain(-6), label: "−6" },
  { value: dbToGain(-12), label: "−12" },
  { value: dbToGain(-24), label: "−24" },
  { value: dbToGain(-48), label: "−48" },
  { value: 0, label: "−∞" },
]

export type PlacedFaderTick = FaderTick & {
  /** Where the mark sits along the travel, 0 to 1. */
  position: number
}

/** The least distance between two labels along the travel, in pixels. */
export const FADER_LABEL_GAP = { vertical: 10, horizontal: 20 }

/**
 * Drops the labels (never the lines) that do not fit a travel of `length`
 * pixels, so that no two labels come closer than `gap` pixels.
 *
 * Labels are kept in order of importance: the strong marks first, then the
 * two ends, then whichever mark lies furthest from every label kept so far.
 * For the dB marks that is 0, then +6 and −∞, then −12, −24, −6 and −48.
 */
export function fitFaderLabels(
  marks: readonly PlacedFaderTick[],
  length: number,
  gap: number
): PlacedFaderTick[] {
  const kept: PlacedFaderTick[] = []
  const room = (mark: PlacedFaderTick) =>
    Math.min(
      Infinity,
      ...kept.map((other) => Math.abs(other.position - mark.position) * length)
    )
  const keep = (mark: PlacedFaderTick | undefined) => {
    if (mark && !kept.includes(mark) && room(mark) >= gap) {
      kept.push(mark)
    }
  }

  const labeled = marks.filter((mark) => mark.label !== undefined)
  const byPosition = [...labeled].sort((a, b) => a.position - b.position)
  for (const mark of labeled) {
    if (mark.strong) {
      keep(mark)
    }
  }
  keep(byPosition[byPosition.length - 1])
  keep(byPosition[0])
  for (;;) {
    let next: PlacedFaderTick | undefined
    for (const mark of byPosition) {
      if (!kept.includes(mark) && (!next || room(mark) > room(next))) {
        next = mark
      }
    }
    if (!next || room(next) < gap) {
      break
    }
    kept.push(next)
  }
  return marks.map((mark) =>
    kept.includes(mark) ? mark : { ...mark, label: undefined }
  )
}

const faderControlVariants = cva(
  "group/fader relative shrink-0 touch-none rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring data-disabled:cursor-default",
  {
    variants: {
      orientation: {
        vertical: "cursor-ns-resize",
        horizontal: "cursor-ew-resize",
      },
      size: { sm: "", md: "" },
    },
    // The size is the thickness of the control across its travel.
    compoundVariants: [
      { orientation: "vertical", size: "sm", className: "w-5" },
      { orientation: "vertical", size: "md", className: "w-7" },
      { orientation: "horizontal", size: "sm", className: "h-5" },
      { orientation: "horizontal", size: "md", className: "h-7" },
    ],
    defaultVariants: { orientation: "vertical", size: "md" },
  }
)

// Length of the cap along its travel, in pixels. The travel is inset by half
// of it at both ends so the cap never leaves the fader.
const CAP = 14
const INSET = CAP / 2

type FaderProps = Omit<
  React.ComponentProps<"div">,
  "defaultValue" | "onChange"
> &
  ValueControlProps & {
    orientation?: "vertical" | "horizontal"
    /** How thick the control is across its travel: 20 or 28 pixels. */
    size?: "sm" | "md"
    /**
     * Scale marks. Defaults to dB marks for a gain fader; false hides them.
     * Labels that do not fit the fader's length are left out.
     */
    ticks?: readonly FaderTick[] | false
    label?: React.ReactNode
    /** Show the value under the fader. Double-click it to type. */
    showValue?: boolean
    /** Rendered beside the travel and aligned to it, usually a `LevelMeter`. */
    meter?: React.ReactNode
    /** Fill color as any CSS color. Defaults to `--wf-control-fill`. */
    color?: string
    /**
     * A value something else is moving the fader to, such as an automation
     * curve while a song plays. The fader shows it with a second cap, drawn
     * outside React, and dims its own. `value` stays what a drag changes.
     */
    live?: LiveValueFeed
    /**
     * A small dot at the fader's corner in this CSS color: something else
     * can move this value. It is also the color of the live cap when the
     * feed gives none.
     */
    marker?: string
  }

/**
 * A level fader. With no `min`, `max` or `scale` it is a gain fader: linear
 * gain 0 to 2.0 on a dB taper, with dB marks, a dB readout and 0 dB as the
 * default. Pass a range to use it for anything else.
 */
function Fader({
  className,
  style,
  orientation = "vertical",
  size = "md",
  ticks,
  label,
  showValue = false,
  meter,
  color,
  live,
  marker,
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
  disabled = false,
  "aria-label": ariaLabel,
  "aria-labelledby": ariaLabelledBy,
  ...props
}: FaderProps) {
  const isGain = min === undefined && max === undefined && scale === undefined
  const low = min ?? 0
  const high = max ?? (isGain ? FADER_MAX_GAIN : 1)
  const taper = scale ?? (isGain ? faderTaper : "linear")
  const vertical = orientation === "vertical"

  const control = useDragValue({
    value,
    onValueChange,
    onGestureStart,
    onGestureEnd,
    defaultValue: defaultValue ?? (isGain ? 1 : undefined),
    min: low,
    max: high,
    step,
    keyStep,
    scale: taper,
    format: format ?? (isGain ? formatGain : undefined),
    parse: parse ?? (isGain ? parseGain : undefined),
    unitKind: unitKind ?? (isGain ? "gain" : undefined),
    disabled,
    orientation,
    live,
    trackOvershoot: true,
    dragRange: (element) =>
      (vertical ? element.clientHeight : element.clientWidth) - CAP,
  })
  const labelId = React.useId()
  const liveCap = React.useRef<HTMLDivElement>(null)
  const liveText = React.useRef<HTMLSpanElement>(null)
  const readout =
    format ??
    (isGain
      ? formatGain
      : (shown: number) => defaultFormat(shown, { min: low, max: high, step }))

  useLiveValue(live, (next, liveColor) => {
    const cap = liveCap.current
    if (!cap) {
      return
    }
    const root = cap.closest<HTMLElement>("[data-orientation]")
    if (next === null) {
      cap.style.display = "none"
      root?.removeAttribute("data-live")
      return
    }
    const shown = clampValue(next, low, high)
    const along = `${(valueToNormalized(shown, low, high, taper) * 100).toFixed(3)}%`
    cap.style.display = ""
    cap.style.setProperty(
      "--live-color",
      liveColor ?? marker ?? "var(--wf-playhead)"
    )
    if (vertical) {
      cap.style.bottom = along
    } else {
      cap.style.left = along
    }
    root?.setAttribute("data-live", "")
    if (liveText.current) {
      liveText.current.textContent = readout(shown)
    }
  })

  // The length of the travel in pixels decides which marks get a label.
  // Until it is measured, all of them do.
  const [travel, setTravel] = React.useState(0)
  const measureTravel = React.useCallback(
    (scale: HTMLDivElement | null) => {
      if (!scale) {
        return undefined
      }
      const measure = () =>
        setTravel(vertical ? scale.clientHeight : scale.clientWidth)
      measure()
      if (typeof ResizeObserver === "undefined") {
        return undefined
      }
      const observer = new ResizeObserver(measure)
      observer.observe(scale)
      return () => observer.disconnect()
    },
    [vertical]
  )

  const placed = (
    ticks === false ? [] : (ticks ?? (isGain ? FADER_DB_TICKS : []))
  ).map((mark) => ({
    ...mark,
    position: valueToNormalized(mark.value, low, high, taper),
  }))
  const marks =
    travel > 0
      ? fitFaderLabels(
          placed,
          travel,
          vertical ? FADER_LABEL_GAP.vertical : FADER_LABEL_GAP.horizontal
        )
      : placed
  const percent = `${(control.normalized * 100).toFixed(3)}%`
  const along = (position: number): React.CSSProperties =>
    vertical
      ? { bottom: `${(position * 100).toFixed(3)}%` }
      : { left: `${(position * 100).toFixed(3)}%` }

  const scaleMarks =
    marks.length > 0 ? (
      <div
        data-slot="fader-scale"
        aria-hidden="true"
        className={cn(
          "relative shrink-0 text-[9px] leading-none text-muted-foreground tabular-nums",
          vertical ? "w-6" : "h-4"
        )}
      >
        <div
          ref={measureTravel}
          className="absolute"
          style={
            vertical ? { inset: `${INSET}px 0` } : { inset: `0 ${INSET}px` }
          }
        >
          {marks.map((mark) => (
            <div
              key={mark.value}
              className={cn(
                "absolute flex items-center gap-0.5",
                mark.strong && "text-foreground",
                vertical
                  ? "right-0 translate-y-1/2"
                  : "top-0 -translate-x-1/2 flex-col"
              )}
              style={along(mark.position)}
            >
              {vertical ? <span>{mark.label}</span> : null}
              <span
                className={cn(
                  mark.strong ? "bg-foreground" : "bg-(--wf-grid-line-strong)",
                  vertical ? "h-px w-1" : "h-1 w-px"
                )}
              />
              {vertical ? null : <span>{mark.label}</span>}
            </div>
          ))}
        </div>
      </div>
    ) : null

  const slider = (
    <div
      {...control.sliderProps}
      data-slot="fader-control"
      aria-label={ariaLabel}
      aria-labelledby={
        ariaLabelledBy ?? (label && !ariaLabel ? labelId : undefined)
      }
      className={faderControlVariants({ orientation, size })}
    >
      {marker ? (
        <span
          data-slot="fader-marker"
          aria-hidden="true"
          className="pointer-events-none absolute top-0 right-0 z-10 size-[5px] rounded-full ring-1 ring-background"
          style={{ backgroundColor: marker }}
        />
      ) : null}
      <div
        className={cn(
          "absolute rounded-full bg-(--wf-control-track)",
          vertical
            ? "inset-y-0 left-1/2 w-1 -translate-x-1/2"
            : "inset-x-0 top-1/2 h-1 -translate-y-1/2"
        )}
      />
      <div
        className="absolute"
        style={vertical ? { inset: `${INSET}px 0` } : { inset: `0 ${INSET}px` }}
      >
        <div
          data-slot="fader-fill"
          className={cn(
            "absolute rounded-full bg-[var(--fader-fill,var(--wf-control-fill))] group-data-live/level:opacity-30",
            vertical
              ? "left-1/2 w-1 -translate-x-1/2"
              : "top-1/2 h-1 -translate-y-1/2"
          )}
          style={
            vertical
              ? { bottom: -INSET, height: `calc(${percent} + ${INSET}px)` }
              : { left: -INSET, width: `calc(${percent} + ${INSET}px)` }
          }
        />
        <div
          data-slot="fader-cap"
          className={cn(
            "absolute rounded-[3px] border border-[color-mix(in_oklch,var(--foreground)_55%,var(--background))] bg-[var(--wf-fader-cap,color-mix(in_oklch,var(--foreground)_86%,var(--background)))] shadow-[0_1px_2px_rgb(0_0_0/0.35)] transition-[background-color] group-hover/fader:bg-[var(--wf-fader-cap-hover,var(--foreground))] group-data-dragging/fader:bg-[var(--wf-fader-cap-hover,var(--foreground))] group-data-live/level:opacity-30",
            vertical
              ? "inset-x-0.5 translate-y-1/2"
              : "inset-y-0.5 -translate-x-1/2"
          )}
          style={
            vertical
              ? { bottom: percent, height: CAP }
              : { left: percent, width: CAP }
          }
        >
          <div
            className={cn(
              "absolute rounded-full bg-[var(--fader-fill,var(--wf-control-fill))]",
              vertical
                ? "inset-x-1 top-1/2 h-0.5 -translate-y-1/2"
                : "inset-y-1 left-1/2 w-0.5 -translate-x-1/2"
            )}
          />
        </div>
        {live ? (
          <div
            ref={liveCap}
            data-slot="fader-live"
            className={cn(
              "pointer-events-none absolute rounded-[3px] border-2 border-(--live-color) bg-[color-mix(in_oklch,var(--live-color)_30%,transparent)]",
              vertical
                ? "inset-x-0.5 translate-y-1/2"
                : "inset-y-0.5 -translate-x-1/2"
            )}
            style={
              vertical
                ? { display: "none", height: CAP }
                : { display: "none", width: CAP }
            }
          />
        ) : null}
      </div>
    </div>
  )

  const meterSlot = meter ? (
    <div
      data-slot="fader-meter"
      className={cn("flex shrink-0", vertical ? "*:h-full" : "*:w-full")}
      style={
        vertical ? { padding: `${INSET}px 0` } : { padding: `0 ${INSET}px` }
      }
    >
      {meter}
    </div>
  ) : null

  return useValueControlSlot(
    control.actions,
    <div
      data-slot="fader"
      data-orientation={orientation}
      data-disabled={disabled ? "" : undefined}
      className={cn(
        "group/level relative inline-flex flex-col items-center gap-1.5 text-[10px] select-none data-disabled:opacity-50",
        vertical ? "h-44" : "w-48",
        className
      )}
      style={
        color
          ? ({ "--fader-fill": color, ...style } as React.CSSProperties)
          : style
      }
      {...props}
    >
      <div
        className={cn(
          "flex min-h-0 min-w-0 flex-1",
          vertical ? "flex-row gap-1" : "w-full flex-col gap-0.5"
        )}
      >
        {vertical ? scaleMarks : meterSlot}
        {slider}
        {vertical ? meterSlot : scaleMarks}
      </div>
      {showValue ? (
        <span
          data-slot="fader-value"
          className="leading-none whitespace-nowrap text-foreground tabular-nums"
          onDoubleClick={() => control.startEditing()}
        >
          {live ? (
            <>
              <span className="group-data-live/level:hidden">
                {control.text}
              </span>
              <span
                ref={liveText}
                data-slot="fader-live-value"
                className="hidden group-data-live/level:inline"
              />
            </>
          ) : (
            control.text
          )}
        </span>
      ) : null}
      {label ? (
        <span
          id={labelId}
          data-slot="fader-label"
          // The padding keeps the tails of g, p and q from being cut off.
          className="-my-[0.25em] max-w-full truncate py-[0.25em] leading-none text-muted-foreground"
        >
          {label}
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

export { Fader, faderControlVariants }
export type { FaderProps }
