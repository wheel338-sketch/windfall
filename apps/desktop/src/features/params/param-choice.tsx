import type { KeyboardEvent, ReactNode } from "react"

import type { ParamChoice, ParamInfo } from "@/bindings"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { cn } from "@/lib/utils"
import { useTranslation } from "@/lib/i18n"

/** Up to this many options are shown side by side; more go into a list. */
export const SEGMENTED_MAX_CHOICES = 4

export type ChoiceIcon = (choice: ParamChoice, index: number) => ReactNode

type ChoiceProps = {
  info: ParamInfo
  /** Index of the chosen option. */
  value: number
  onChoose(index: number): void
  disabled: boolean
  icon?: ChoiceIcon
  className?: string
}

/** Every option as a button in one strip, the chosen one raised. */
export function SegmentedChoice({
  info,
  value,
  onChoose,
  disabled,
  icon,
  className,
}: ChoiceProps) {
  const t = useTranslation()
  const count = info.choices.length

  // Arrows move the choice, as they do in a group of radio buttons.
  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return
    let next: number
    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      next = (value + 1) % count
    } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      next = (value + count - 1) % count
    } else if (event.key === "Home") {
      next = 0
    } else if (event.key === "End") {
      next = count - 1
    } else {
      return
    }
    event.preventDefault()
    onChoose(next)
    event.currentTarget
      .querySelector<HTMLElement>(`[data-choice="${next}"]`)
      ?.focus()
  }

  return (
    <div
      role="radiogroup"
      aria-label={t(info.name)}
      aria-disabled={disabled || undefined}
      data-slot="param-segments"
      onKeyDown={onKeyDown}
      className={cn(
        "inline-flex max-w-full min-w-0 rounded-[5px] bg-(--wf-step-off)/70 p-px",
        disabled && "opacity-50",
        className
      )}
    >
      {info.choices.map((choice, index) => {
        const chosen = index === value
        return (
          <button
            key={choice.value}
            type="button"
            role="radio"
            aria-checked={chosen}
            data-choice={index}
            disabled={disabled}
            tabIndex={chosen ? 0 : -1}
            onClick={() => onChoose(index)}
            className={cn(
              "flex h-5 min-w-0 flex-auto items-center justify-center gap-1 rounded-[4px] px-1.5 text-[0.625rem] leading-none whitespace-nowrap text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring",
              chosen && "bg-background text-foreground shadow-xs"
            )}
          >
            {icon?.(choice, index)}
            <span className="truncate">{t(choice.label)}</span>
          </button>
        )
      })}
    </div>
  )
}

/** The chosen option on a button that opens the list of all of them. */
export function SelectChoice({
  info,
  value,
  onChoose,
  disabled,
  icon,
  className,
}: ChoiceProps) {
  const t = useTranslation()
  const items = info.choices.map((choice, index) => ({
    value: index,
    label: t(choice.label),
  }))
  const chosen = info.choices[value]

  return (
    <Select
      items={items}
      value={value}
      disabled={disabled}
      onValueChange={(next: number | null) => {
        if (next !== null) onChoose(next)
      }}
    >
      <SelectTrigger
        size="sm"
        aria-label={t(info.name)}
        data-slot="param-select"
        className={cn(
          "h-[22px] w-full min-w-0 gap-1 rounded-[5px] py-0 pr-1 pl-1.5 text-[0.6875rem]",
          className
        )}
      >
        {chosen && icon?.(chosen, value)}
        <SelectValue className="min-w-0 truncate" />
      </SelectTrigger>
      <SelectContent alignItemWithTrigger={false} className="min-w-36">
        <SelectGroup>
          {info.choices.map((choice, index) => (
            <SelectItem key={choice.value} value={index}>
              {icon?.(choice, index)}
              {t(choice.label)}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}
