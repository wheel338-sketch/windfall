import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import { useId, type ReactNode } from "react"

import { Switch } from "@/components/ui/switch"
import { useHint } from "@/lib/store/hint"
import { cn } from "@/lib/utils"

type SectionProps = {
  title: string
  /** Shown at the right of the title, such as an on/off switch. */
  aside?: ReactNode
  className?: string
  children?: ReactNode
}

/** One titled block of the channel settings. */
export function Section({ title, aside, className, children }: SectionProps) {
  useLanguageStore((state) => state.language)
  const id = useId()
  return (
    <section aria-labelledby={id} className="border-b px-2.5 py-2">
      <div className="mb-1.5 flex h-5 items-center justify-between gap-2">
        <h3 id={id} className="text-xs font-medium">
          {t(title)}
        </h3>
        {aside}
      </div>
      <div className={cn("flex flex-col gap-2", className)}>{children}</div>
    </section>
  )
}

type SwitchRowProps = {
  label: string
  hint: string
  checked: boolean
  onCheckedChange(checked: boolean): void
}

/** A labelled on/off switch. */
export function SwitchRow({
  label,
  hint,
  checked,
  onCheckedChange,
}: SwitchRowProps) {
  useLanguageStore((state) => state.language)
  const id = useId()
  const hintProps = useHint(t(hint))
  return (
    <div className="flex h-5 items-center justify-between gap-2" {...hintProps}>
      <label htmlFor={id} className="truncate text-foreground/85">
        {t(label)}
      </label>
      <Switch
        id={id}
        size="sm"
        checked={checked}
        onCheckedChange={(next) => onCheckedChange(next)}
      />
    </div>
  )
}
