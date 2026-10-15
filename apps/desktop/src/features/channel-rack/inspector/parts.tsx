import { message, translatedText as t, useLanguageStore } from "@/lib/i18n"
import { ArrowDown01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { useId, useState, type ReactNode } from "react"

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

/**
 * Sections folded away, by title. Kept for the session, so a section folded
 * on one channel stays folded on the next.
 */
const folded = new Set<string>()

/**
 * One titled block of the channel settings. Its title folds it away, and
 * the settings' section bar finds it by `data-inspector-section`.
 */
export function Section({ title, aside, className, children }: SectionProps) {
  useLanguageStore((state) => state.language)
  const id = useId()
  const [open, setOpen] = useState(() => !folded.has(title))
  const name = t(title)
  function toggle() {
    if (open) folded.add(title)
    else folded.delete(title)
    setOpen(!open)
  }
  return (
    <section
      aria-labelledby={id}
      data-inspector-section={name}
      className="border-b px-2.5 py-2"
    >
      <div
        className={cn(
          "flex h-5 items-center justify-between gap-2",
          open && "mb-1.5"
        )}
      >
        <span className="flex min-w-0 items-center gap-1">
          <button
            type="button"
            aria-expanded={open}
            aria-label={
              open
                ? message("Fold {section}", { section: name })
                : message("Unfold {section}", { section: name })
            }
            onClick={toggle}
            className="-ml-1 flex size-4 shrink-0 items-center justify-center rounded-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
          >
            <HugeiconsIcon
              icon={ArrowDown01Icon}
              strokeWidth={2}
              className={cn("size-3 transition-transform", !open && "-rotate-90")}
            />
          </button>
          <h3 id={id} className="truncate text-xs font-medium" title={name}>
            {name}
          </h3>
        </span>
        {aside}
      </div>
      {open && (
        <div className={cn("flex flex-col gap-2", className)}>{children}</div>
      )}
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
