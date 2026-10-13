// SPDX-License-Identifier: MIT
import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

import {
  clampValue,
  snapValue,
  useDragValue,
  useValueControlSlot,
  type ValueControlProps,
} from "./use-drag-value"
import { ValueInput } from "./value-input"
import { translatedText as t } from "@/lib/i18n"

const numberFieldVariants = cva(
  "group/number relative inline-flex shrink-0 items-center rounded-md border border-input bg-input/20 text-foreground tabular-nums outline-none select-none has-focus-visible:border-ring has-focus-visible:ring-2 has-focus-visible:ring-ring/30 dark:bg-input/30 data-disabled:opacity-50",
  {
    variants: {
      size: {
        sm: "h-6 px-1.5 text-xs",
        md: "h-7 px-2 text-sm",
        lg: "h-9 px-2.5 text-lg font-medium",
      },
    },
    defaultVariants: { size: "md" },
  }
)

type NumberFieldProps = Omit<
  React.ComponentProps<"div">,
  "defaultValue" | "onChange"
> &
  Omit<ValueControlProps, "scale" | "format"> &
  VariantProps<typeof numberFieldVariants> & {
    /** Digits shown after the decimal point. Defaults to those of `step`. */
    decimals?: number
    /**
     * Drag the whole-number part and the decimals at different rates:
     * dragging the digits before the point moves by `coarseStep`, dragging
     * the digits after it moves by `step`.
     */
    splitDrag?: boolean
    /** What a drag on the whole-number part moves by. Defaults to 1. */
    coarseStep?: number
    /** Pointer travel in pixels for one step of the dragged part. */
    pixelsPerStep?: number
    /** Shown after the number, such as "BPM". */
    unit?: React.ReactNode
  }

function decimalsOfStep(step: number | undefined): number {
  if (!step) {
    return 0
  }
  const text = String(step)
  const dot = text.indexOf(".")
  return dot < 0 ? 0 : text.length - dot - 1
}

/**
 * A compact number that is dragged, typed or nudged with the wheel, as for a
 * tempo. Drag up and down to change it, double-click or press Enter to type.
 */
function NumberField({
  className,
  size = "md",
  decimals,
  splitDrag = false,
  coarseStep = 1,
  pixelsPerStep = 4,
  unit,
  value,
  onValueChange,
  onGestureStart,
  onGestureEnd,
  defaultValue,
  min = 0,
  max = 100,
  step = 1,
  parse,
  unitKind,
  disabled = false,
  "aria-label": ariaLabel,
  "aria-labelledby": ariaLabelledBy,
  ...props
}: NumberFieldProps) {
  const digits = decimals ?? decimalsOfStep(step)
  const coarse = Math.max(coarseStep, step)
  // The drag in progress: what one notch of travel moves by, whether it
  // moves in whole notches, and the value it started from.
  const stroke = React.useRef({ by: coarse, whole: false, from: value })
  const stroking = React.useRef(false)

  const control = useDragValue({
    value,
    onGestureStart,
    onGestureEnd,
    defaultValue,
    min,
    max,
    step,
    keyStep: coarse,
    parse,
    unitKind,
    disabled,
    format: (current) => current.toFixed(digits),
    doubleClick: "edit",
    dragRange: () => ((max - min) / stroke.current.by) * pixelsPerStep,
    onValueChange: (next) => {
      const { by, whole, from } = stroke.current
      if (!stroking.current || !whole) {
        onValueChange?.(next)
        return
      }
      // A drag on the whole-number part moves in whole coarse steps and
      // leaves the decimals as they were.
      const moved = from + Math.round((next - from) / by) * by
      const snapped = snapValue(clampValue(moved, min, max), min, max, step)
      if (snapped !== value) {
        onValueChange?.(snapped)
      }
    },
  })

  const [whole, fraction] = control.text.split(".")

  return useValueControlSlot(
    control.actions,
    <div
      data-slot="number-field"
      data-size={size}
      data-disabled={disabled ? "" : undefined}
      className={cn(numberFieldVariants({ size }), className)}
      {...props}
    >
      <div
        {...control.sliderProps}
        data-slot="number-field-control"
        aria-label={ariaLabel}
        aria-labelledby={ariaLabelledBy}
        className={cn(
          "flex h-full cursor-ns-resize touch-none items-center outline-none data-disabled:cursor-default",
          control.editing && "invisible"
        )}
        onPointerDownCapture={(event) => {
          const part = (event.target as HTMLElement).closest<HTMLElement>(
            "[data-part]"
          )?.dataset.part
          const fine = splitDrag && part === "fraction"
          stroke.current = {
            by: fine ? step : coarse,
            whole: splitDrag && !fine,
            from: control.value,
          }
          stroking.current = true
        }}
        onPointerUpCapture={() => {
          stroking.current = false
        }}
        onPointerCancelCapture={() => {
          stroking.current = false
        }}
      >
        <span
          data-part="whole"
          className={cn("rounded-[2px]", splitDrag && "hover:bg-foreground/10")}
        >
          {whole}
        </span>
        {fraction !== undefined ? (
          <span
            data-part="fraction"
            className={cn(
              "rounded-[2px] text-muted-foreground",
              splitDrag && "hover:bg-foreground/10"
            )}
          >
            .{fraction}
          </span>
        ) : null}
        {unit ? (
          <span
            data-slot="number-field-unit"
            className="ml-1 text-[0.75em] font-normal text-muted-foreground"
          >
            {unit}
          </span>
        ) : null}
      </div>
      {control.editing ? (
        <ValueInput
          {...control.entryProps}
          aria-label={ariaLabel ?? t("Value")}
          className="absolute inset-0 size-full rounded-[inherit] border-0 px-1 text-[length:inherit] ring-0"
        />
      ) : null}
    </div>
  )
}

export { NumberField, numberFieldVariants }
export type { NumberFieldProps }
