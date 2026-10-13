import { ArrowDown01Icon, ArrowRight01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { useEffect, useRef, useState } from "react"

import type { HistoryEntry } from "@/bindings"
import { Empty, EmptyDescription } from "@/components/ui/empty"
import { historyJump } from "@/lib/store/project"
import { useHistory } from "@/lib/store/selectors"
import { cn } from "@/lib/utils"

import { matchingRuns } from "./history-filter"
import { message, translatedText as t, plainNumber } from "@/lib/i18n"

type StepState = "done" | "current" | "undone"

/** Edits in a row that carry the same label, such as sixteen toggled steps. */
export type HistoryRun = {
  label: string
  /** How many entries are applied after the first and the last of them. */
  first: number
  last: number
}

export function groupRuns(entries: HistoryEntry[]): HistoryRun[] {
  const runs: HistoryRun[] = []
  entries.forEach((entry, index) => {
    const previous = runs.at(-1)
    if (previous?.label === entry.label) previous.last = index + 1
    else runs.push({ label: entry.label, first: index + 1, last: index + 1 })
  })
  return runs
}

function stateAt(applied: number, cursor: number): StepState {
  if (applied === cursor) return "current"
  return applied < cursor ? "done" : "undone"
}

function Dot({ state }: { state: StepState }) {
  return (
    <span
      aria-hidden
      className={cn(
        "size-1.5 shrink-0 rounded-full",
        state === "current" && "bg-brand",
        state === "done" && "bg-foreground/25",
        state === "undone" && "ring-1 ring-foreground/25 ring-inset"
      )}
    />
  )
}

type HistoryRowProps = {
  label: string
  /** How many entries are applied once this row is chosen. */
  cursor: number
  state: StepState
  /** For a row that stands for several steps: how many. */
  count?: number
  className?: string
}

function HistoryRow({
  label,
  cursor,
  state,
  count,
  className,
}: HistoryRowProps) {
  return (
    <button
      type="button"
      aria-current={state === "current" ? "step" : undefined}
      onClick={() => void historyJump(cursor)}
      className={cn(
        "flex h-6 min-w-0 flex-1 items-center gap-2 rounded-sm px-2 text-left text-xs outline-none hover:bg-accent focus-visible:bg-accent",
        state === "undone" && "text-muted-foreground/70",
        state === "current" && "font-medium",
        className
      )}
    >
      <Dot state={state} />
      <span className="truncate">{label}</span>
      {count !== undefined && (
        <span className="shrink-0 font-readout text-[0.625rem] text-muted-foreground">
          ×{count}
        </span>
      )}
    </button>
  )
}

/**
 * A run of steps with one label, folded into a single row. Choosing the row
 * goes to the end of the run; the arrow unfolds it into its steps.
 */
function RunRows({ run, cursor }: { run: HistoryRun; cursor: number }) {
  const [unfolded, setUnfolded] = useState(false)
  // With the current step inside the run, the steps are what matters.
  const inside = cursor >= run.first && cursor < run.last
  const open = unfolded || inside
  const count = run.last - run.first + 1
  const state: StepState =
    cursor > run.last ? "done" : cursor < run.first ? "undone" : "current"

  return (
    <li>
      <div className="flex items-center">
        <button
          type="button"
          aria-expanded={open}
          aria-label={message("{open} the {count} steps of {runLabel}", { open: open ? t("Fold") : t("Unfold"), count: plainNumber(count), runLabel: run.label })}
          // Open because the current step is inside: there is no folding it.
          disabled={inside}
          onClick={() => setUnfolded(!open)}
          className="flex size-6 shrink-0 items-center justify-center rounded-sm text-muted-foreground outline-none hover:bg-accent focus-visible:bg-accent disabled:opacity-40"
        >
          <HugeiconsIcon
            icon={open ? ArrowDown01Icon : ArrowRight01Icon}
            strokeWidth={2}
            className="size-3"
          />
        </button>
        <HistoryRow
          label={run.label}
          cursor={run.last}
          state={open && state === "current" ? "done" : state}
          count={count}
          className="pl-1"
        />
      </div>
      {open && (
        <ol
          aria-label={message("Steps of {runLabel}", { runLabel: run.label })}
          className="flex flex-col gap-px"
        >
          {Array.from({ length: count }, (_, index) => (
            <li key={index} className="flex pl-6">
              <HistoryRow
                label={run.label}
                cursor={run.first + index}
                state={stateAt(run.first + index, cursor)}
              />
            </li>
          ))}
        </ol>
      )}
    </li>
  )
}

/**
 * The undo history, oldest first. Choosing a row undoes or redoes up to it.
 * Steps in a row with the same label are folded into one. Self-contained,
 * so it can sit in a popover or a panel of its own.
 */
export function HistoryList({
  className,
  query = "",
}: {
  className?: string
  query?: string
}) {
  const { entries, cursor } = useHistory()
  const runs = matchingRuns(
    [{ label: t("Project opened"), first: 0, last: 0 }, ...groupRuns(entries)],
    query
  )
  const list = useRef<HTMLOListElement>(null)
  const shown = useRef(false)

  // The current step is what the list is opened for, and it can be far
  // down. Afterwards it is only kept in view.
  useEffect(() => {
    list.current
      ?.querySelector("[aria-current='step']")
      ?.scrollIntoView({ block: shown.current ? "nearest" : "center" })
    shown.current = true
  }, [cursor, query])

  if (runs.length === 0) {
    return (
      <Empty className="p-2" role="status">
        <EmptyDescription>{t("No steps match.")}</EmptyDescription>
      </Empty>
    )
  }

  return (
    <ol
      ref={list}
      aria-label={t("Undo history")}
      className={cn("flex flex-col gap-px overflow-y-auto", className)}
    >
      {runs.map((run) =>
        run.first === run.last ? (
          <li key={run.first} className="flex">
            <HistoryRow
              label={run.label}
              cursor={run.first}
              state={stateAt(run.first, cursor)}
            />
          </li>
        ) : (
          <RunRows key={run.first} run={run} cursor={cursor} />
        )
      )}
    </ol>
  )
}
