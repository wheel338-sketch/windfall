// SPDX-License-Identifier: MIT
import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"
import { translatedText as t } from "@/lib/i18n"

const toggleLedVariants = cva(
  "inline-flex shrink-0 items-center justify-center font-semibold outline-none select-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        // A lettered button that fills with its color when on (M, S, arm).
        button:
          "rounded-[3px] border border-(--wf-grid-line-strong) bg-(--wf-step-off) leading-none text-muted-foreground transition-colors hover:text-foreground aria-pressed:border-transparent aria-pressed:bg-(--led-color) aria-pressed:text-[var(--led-foreground,oklch(0.18_0_0))]",
        // A small round light, as on a channel's mute switch.
        dot: "rounded-full border border-(--wf-grid-line-strong) bg-(--wf-step-off) transition-colors hover:border-foreground/50 aria-pressed:border-transparent aria-pressed:bg-(--led-color) aria-pressed:shadow-[0_0_6px_var(--led-color)]",
      },
      size: {
        sm: "",
        md: "",
        lg: "",
      },
    },
    compoundVariants: [
      { variant: "button", size: "sm", className: "size-4 text-[9px]" },
      { variant: "button", size: "md", className: "size-5 text-[10px]" },
      { variant: "button", size: "lg", className: "size-6 text-[11px]" },
      { variant: "dot", size: "sm", className: "size-2" },
      { variant: "dot", size: "md", className: "size-2.5" },
      { variant: "dot", size: "lg", className: "size-3.5" },
    ],
    defaultVariants: { variant: "button", size: "md" },
  }
)

type ToggleLedProps = Omit<React.ComponentProps<"button">, "onChange"> &
  VariantProps<typeof toggleLedVariants> & {
    pressed: boolean
    onPressedChange?: (pressed: boolean) => void
    /** The lit color as any CSS color. Defaults to `--wf-brand`. */
    color?: string
  }

/** A small latching button that lights up when on: mute, solo, arm. */
function ToggleLed({
  className,
  style,
  variant = "button",
  size = "md",
  pressed,
  onPressedChange,
  color = "var(--wf-brand)",
  onClick,
  type = "button",
  ...props
}: ToggleLedProps) {
  return (
    <button
      type={type}
      data-slot="toggle-led"
      data-variant={variant}
      aria-pressed={pressed}
      className={cn(toggleLedVariants({ variant, size }), className)}
      style={{ "--led-color": color, ...style } as React.CSSProperties}
      onClick={(event) => {
        onClick?.(event)
        if (!event.defaultPrevented) {
          onPressedChange?.(!pressed)
        }
      }}
      {...props}
    />
  )
}

type MuteSoloProps = Omit<React.ComponentProps<"div">, "onChange"> & {
  muted: boolean
  solo: boolean
  onMutedChange?: (muted: boolean) => void
  onSoloChange?: (solo: boolean) => void
  size?: "sm" | "md" | "lg"
  disabled?: boolean
}

/** The mute and solo pair of a mixer strip or channel. */
function MuteSolo({
  className,
  muted,
  solo,
  onMutedChange,
  onSoloChange,
  size = "md",
  disabled,
  ...props
}: MuteSoloProps) {
  return (
    <div
      data-slot="mute-solo"
      className={cn("inline-flex items-center gap-0.5", className)}
      {...props}
    >
      <ToggleLed
        size={size}
        pressed={muted}
        onPressedChange={onMutedChange}
        color="var(--wf-mute, var(--wf-meter-mid))"
        disabled={disabled}
        aria-label={t("Mute")}
      >
        M
      </ToggleLed>
      <ToggleLed
        size={size}
        pressed={solo}
        onPressedChange={onSoloChange}
        color="var(--wf-solo, var(--wf-meter-low))"
        disabled={disabled}
        aria-label={t("Solo")}
      >
        S
      </ToggleLed>
    </div>
  )
}

export { ToggleLed, MuteSolo, toggleLedVariants }
export type { ToggleLedProps, MuteSoloProps }
