import type { ReactNode } from "react"

import { Fader } from "@/components/audio"
import { ValueContextItems } from "@/components/value-context-menu"
import {
  formatParamDisplay,
  paramHint,
  paramScale,
  paramUnitKind,
  parseParamDisplay,
  type ParamBinding,
} from "@/features/params"
import { useHint } from "@/lib/store/hint"
import { cn } from "@/lib/utils"
import { useTranslation } from "@/lib/i18n"

type ParamFaderProps = {
  /** What `bind(id)` gives for the setting. */
  binding: ParamBinding
  /** Shown under the fader instead of the setting's name. */
  label?: ReactNode
  /** One sentence for the status bar about what the setting does. */
  description?: string
  orientation?: "vertical" | "horizontal"
  className?: string
}

/**
 * A setting on a fader instead of a knob, for the editors where a row of
 * levels reads as a shape: the bands of a graphic equalizer, the levels of
 * a filter's outputs. It is bound, automated and described the way a
 * `ParamControl` is; only the control is different.
 */
export function ParamFader({
  binding,
  label,
  description,
  orientation = "vertical",
  className,
}: ParamFaderProps) {
  const t = useTranslation()
  const { info, value, contextItems } = binding
  const hint = useHint(paramHint(info, value, description))
  const fader = (
    <Fader
      data-param={info.id}
      orientation={orientation}
      size="sm"
      ticks={false}
      aria-label={t(info.name)}
      showValue
      min={info.min}
      max={info.max}
      scale={paramScale(info)}
      defaultValue={info.default}
      format={(next) => formatParamDisplay(info, next)}
      parse={(typed) => parseParamDisplay(info, typed)}
      unitKind={paramUnitKind(info)}
      value={value}
      onValueChange={binding.onValueChange}
      onGestureStart={binding.onGestureStart}
      onGestureEnd={binding.onGestureEnd}
      disabled={binding.disabled}
      live={binding.live}
      marker={binding.marker}
      className={cn(orientation === "vertical" ? "h-28" : "w-full", className)}
      {...hint}
    />
  )
  return (
    <div
      data-slot="param-fader"
      className="flex min-w-0 flex-col items-center gap-1 text-[10px]"
    >
      {contextItems.length > 0 ? (
        <ValueContextItems items={contextItems}>{fader}</ValueContextItems>
      ) : (
        fader
      )}
      {label === null ? null : (
        <span className="max-w-full truncate leading-none text-muted-foreground">
          {label === undefined ? t(info.name) : typeof label === "string" ? t(label) : label}
        </span>
      )}
    </div>
  )
}
