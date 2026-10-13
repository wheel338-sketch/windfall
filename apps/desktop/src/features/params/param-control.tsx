import { memo, type MouseEvent, type ReactElement, type ReactNode } from "react"

import type { ParamInfo } from "@/bindings"
import {
  Knob,
  ToggleLed,
  type LiveValueFeed,
  type ValueScaleOption,
} from "@/components/audio"
import { ContextActions, type ContextItem } from "@/components/context-actions"
import { Switch } from "@/components/ui/switch"
import { ValueContextItems } from "@/components/value-context-menu"
import { useHint } from "@/lib/store/hint"
import { cn } from "@/lib/utils"
import { message, translatedText, useTranslation } from "@/lib/i18n"

import { clampParam } from "./access"
import {
  formatParamDisplay,
  paramIsBipolar,
  paramScale,
  paramUnitKind,
  parseParamDisplay,
} from "./format"
import {
  SEGMENTED_MAX_CHOICES,
  SegmentedChoice,
  SelectChoice,
  type ChoiceIcon,
} from "./param-choice"

export type ParamControlSize = "sm" | "md" | "lg"

export type ParamControlProps = {
  /** The setting, from a descriptor. It picks the control. */
  info: ParamInfo
  /** The value as `readParam` gives it. */
  value: number
  onValueChange?: (value: number) => void
  /** Called before the first change of a drag, a click or a typed entry. */
  onGestureStart?: () => void
  onGestureEnd?: () => void
  size?: ParamControlSize
  /**
   * Shown instead of the setting's name, for a shorter word inside a group
   * that already says the rest. Pass null to show no label. The accessible
   * name and the status bar keep the full name.
   */
  label?: ReactNode
  disabled?: boolean
  /** One sentence for the status bar about what the setting does. */
  description?: string
  /** The arc of a knob or the light of a toggle, as any CSS color. */
  color?: string
  /**
   * How a toggle or a choice sits next to its label: "stacked" under or
   * over it, like a knob, or "inline" with the label on the left and the
   * control on the right, filling the row.
   */
  layout?: "stacked" | "inline"
  /** A choice is a strip of buttons up to four options, a list above. */
  choiceStyle?: "auto" | "segmented" | "select"
  /** Drawn before the name of each option of a choice. */
  choiceIcon?: ChoiceIcon
  /** Replaces the knob travel the descriptor asks for. */
  scale?: ValueScaleOption
  /**
   * Entries about the setting itself for the right-click menu of its knob,
   * put before the ones every value control has. `bind(id)` supplies them.
   */
  contextItems?: readonly ContextItem[]
  /**
   * The value something else is giving the setting right now, such as an
   * automation curve. A knob shows it beside its own. `bind(id)` supplies it.
   */
  live?: LiveValueFeed
  /** The color of the dot that says the setting has an automation. */
  marker?: string
  className?: string
}

const CELL_HEIGHT: Record<ParamControlSize, string> = {
  sm: "h-6",
  md: "h-9",
  lg: "h-13",
}

/** What the status bar says about a setting while its control is pointed at. */
export function paramHint(
  info: ParamInfo,
  value: number,
  description?: string
): string {
  const values = { name: translatedText(info.name), value: formatParamDisplay(info, value) }
  if (description) return message("{name}: {value}. {description}", { ...values, description: translatedText(description) })
  if (info.kind === "toggle") {
    return message(value >= 0.5 ? "{name}: {value}. Click to turn it off" : "{name}: {value}. Click to turn it on", values)
  }
  if (info.kind === "choice") return message("{name}: {value}. Click to choose another", values)
  return message("{name}: {value}. Drag up or down, Shift for fine, double-click for {default}", { ...values, default: formatParamDisplay(info, info.default) })
}

/**
 * The control for one setting of an effect or an instrument, picked from
 * its description: a knob for a number, a light for a toggle, and a strip of
 * buttons or a list for a choice. Each shows the setting's name and its
 * value with the unit, explains itself in the status bar, returns to the
 * default on a double-click and reports its changes inside a gesture.
 */
export const ParamControl = memo(function ParamControl({
  info,
  value,
  onValueChange,
  onGestureStart,
  onGestureEnd,
  size = "md",
  label,
  disabled = false,
  description,
  color,
  layout = "stacked",
  choiceStyle = "auto",
  choiceIcon,
  scale,
  contextItems,
  live,
  marker,
  className,
}: ParamControlProps) {
  const t = useTranslation()
  const shown = clampParam(info, value)
  const hint = useHint(paramHint(info, shown, description))
  const text = label === undefined ? t(info.name) : typeof label === "string" ? t(label) : label

  // A click is a whole gesture: one undo step.
  function commit(next: number) {
    if (disabled || next === shown) return
    onGestureStart?.()
    onValueChange?.(next)
    onGestureEnd?.()
  }
  const reset = () => commit(info.default)
  // Ctrl or Cmd with a click resets, as on a knob. Caught on the way down
  // so the control under the pointer does not also take the click.
  const resetOnModifiedClick = (event: MouseEvent) => {
    if (!event.ctrlKey && !event.metaKey) return
    event.preventDefault()
    event.stopPropagation()
    reset()
  }

  if (info.kind === "float" || info.kind === "integer") {
    const knob = (
      <Knob
        data-slot="param-control"
        data-param={info.id}
        size={size}
        label={text ?? undefined}
        aria-label={t(info.name)}
        showValue
        min={info.min}
        max={info.max}
        step={info.kind === "integer" ? 1 : undefined}
        scale={scale ?? paramScale(info)}
        bipolar={paramIsBipolar(info)}
        center={0}
        defaultValue={info.default}
        format={(next) => formatParamDisplay(info, next)}
        parse={(typed) => parseParamDisplay(info, typed)}
        unitKind={paramUnitKind(info)}
        value={shown}
        onValueChange={onValueChange}
        onGestureStart={onGestureStart}
        onGestureEnd={onGestureEnd}
        disabled={disabled}
        color={color}
        live={live}
        marker={marker}
        // A readout wider than its column spills over evenly. Wrapping it
        // would change the height of the row while the knob is turned.
        className={cn(
          "[&>[data-slot=knob-label]]:max-w-20 [&>[data-slot=knob-value]]:whitespace-nowrap",
          className
        )}
        {...hint}
      />
    )
    // What the setting is bound to adds its entries to the knob's menu.
    return contextItems && contextItems.length > 0 ? (
      <ValueContextItems items={contextItems}>{knob}</ValueContextItems>
    ) : (
      knob
    )
  }

  // A toggle or a choice is no value control, so it has no menu of its
  // own. What its setting is bound to still gets one: this is how a switch
  // or a list of shapes is automated.
  const hasMenu = contextItems !== undefined && contextItems.length > 0
  const withMenu = (control: ReactElement) =>
    hasMenu ? (
      <ContextActions items={() => [...contextItems]}>{control}</ContextActions>
    ) : (
      control
    )
  const markerDot =
    marker === undefined ? null : (
      <span
        data-slot="param-marker"
        aria-hidden
        className="pointer-events-none absolute -top-0.5 -right-0.5 size-[5px] rounded-full ring-1 ring-background"
        style={{ backgroundColor: marker }}
      />
    )

  const labelNode =
    text === null ? null : (
      <span
        data-slot="param-label"
        className={cn(
          // A line as tall as its letters would cut the tails off g, p
          // and q. The padding gives them room, the margin takes it back.
          "-my-[0.25em] truncate py-[0.25em] leading-none text-muted-foreground",
          layout === "stacked" ? "max-w-20" : "min-w-0 text-[0.6875rem]"
        )}
        onDoubleClick={reset}
      >
        {text}
      </span>
    )

  if (info.kind === "toggle") {
    const on = shown >= 0.5
    if (layout === "inline") {
      return withMenu(
        <div
          data-slot="param-control"
          data-param={info.id}
          className={cn(
            "relative flex h-5 min-w-0 items-center justify-between gap-2",
            disabled && "opacity-50",
            className
          )}
          onClickCapture={resetOnModifiedClick}
          {...hint}
        >
          {labelNode}
          <Switch
            size="sm"
            aria-label={t(info.name)}
            checked={on}
            disabled={disabled}
            onCheckedChange={(next) => commit(next ? 1 : 0)}
          />
          {markerDot}
        </div>
      )
    }
    return withMenu(
      <div
        data-slot="param-control"
        data-param={info.id}
        className={cn(
          "relative inline-flex flex-col items-center gap-1 text-[10px] select-none",
          disabled && "opacity-50",
          className
        )}
        onClickCapture={resetOnModifiedClick}
        {...hint}
      >
        {markerDot}
        <span className={cn("flex items-center", CELL_HEIGHT[size])}>
          <ToggleLed
            variant="dot"
            size="lg"
            aria-label={t(info.name)}
            pressed={on}
            disabled={disabled}
            color={color}
            onPressedChange={(next) => commit(next ? 1 : 0)}
          />
        </span>
        {labelNode}
        <span
          data-slot="param-value"
          className="leading-none text-foreground tabular-nums"
        >
          {formatParamDisplay(info, shown)}
        </span>
      </div>
    )
  }

  const segmented =
    choiceStyle === "segmented" ||
    (choiceStyle === "auto" && info.choices.length <= SEGMENTED_MAX_CHOICES)
  const Choice = segmented ? SegmentedChoice : SelectChoice
  return withMenu(
    <div
      data-slot="param-control"
      data-param={info.id}
      className={cn(
        "relative min-w-0 text-[10px]",
        layout === "inline"
          ? "flex items-center justify-between gap-2"
          : "inline-flex flex-col gap-1",
        className
      )}
      onClickCapture={resetOnModifiedClick}
      {...hint}
    >
      {labelNode}
      <Choice
        info={info}
        value={shown}
        onChoose={commit}
        disabled={disabled}
        icon={choiceIcon}
        className={layout === "inline" ? "w-auto shrink-0" : "w-full"}
      />
      {markerDot}
    </div>
  )
})
