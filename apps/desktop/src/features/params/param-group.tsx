import type { ComponentProps, CSSProperties, ReactNode } from "react"

import { cn } from "@/lib/utils"
import { useTranslation } from "@/lib/i18n"

type ParamGroupProps = Omit<ComponentProps<"section">, "title"> & {
  title: string
  /** Shown at the right of the title, such as an on/off switch. */
  aside?: ReactNode
}

/**
 * A titled box for the controls that belong together: one oscillator, the
 * filter, one band of an equalizer. Lay rows of controls out inside it with
 * `ParamRow`.
 */
export function ParamGroup({
  title,
  aside,
  className,
  children,
  ...props
}: ParamGroupProps) {
  const t = useTranslation()
  return (
    <section
      data-slot="param-group"
      aria-label={t(title)}
      className={cn(
        "flex min-w-0 flex-col rounded-md border border-border/80 bg-chassis/50",
        className
      )}
      {...props}
    >
      <div className="flex h-6 shrink-0 items-center justify-between gap-2 pr-1.5 pl-2">
        <h4 className="truncate text-[0.6875rem] font-medium text-foreground/80">
          {t(title)}
        </h4>
        {aside}
      </div>
      <div className="flex min-w-0 flex-col gap-2.5 px-2 pt-0.5 pb-2">
        {children}
      </div>
    </section>
  )
}

type ParamRowProps = ComponentProps<"div"> & {
  /**
   * Lays the controls out in this many equal columns. Without it they sit
   * side by side and wrap when the row is full.
   */
  columns?: number
}

/** One row of controls in a `ParamGroup`. */
export function ParamRow({
  columns,
  className,
  style,
  children,
  ...props
}: ParamRowProps) {
  const grid: CSSProperties | undefined = columns
    ? { gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }
    : undefined
  return (
    <div
      data-slot="param-row"
      className={cn(
        columns
          ? "grid justify-items-center gap-x-1 gap-y-2"
          : "flex flex-wrap items-start gap-x-3 gap-y-2",
        className
      )}
      style={{ ...grid, ...style }}
      {...props}
    >
      {children}
    </div>
  )
}
