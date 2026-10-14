import { Add01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { memo, useState, type KeyboardEvent } from "react"
import { useShallow } from "zustand/react/shallow"

import type { PatternId } from "@/bindings"
import { ActionButton } from "@/components/action-button"
import {
  ContextActions,
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import { runAction } from "@/lib/actions"
import { useHint } from "@/lib/store/hint"
import { useProjectStore } from "@/lib/store/project"
import {
  usePattern,
  useSelectedPatternId,
  useSettings,
} from "@/lib/store/selectors"
import { setTransportPattern } from "@/lib/store/transport"
import { ticksPerBar } from "@/lib/time"
import { colorToCss, TICKS_PER_STEP } from "@/lib/units"
import { cn } from "@/lib/utils"

import { AudioSection } from "./audio/picker-section"
import { AutomationSection } from "./automation/picker-section"
import { PICKER_WIDTH } from "./layout"
import { matchingPatternIds } from "./pattern-filter"
import { PickerEmpty, PickerSection } from "./picker-section"
import { useClipCounts } from "./selectors"
import { usePlaylistStore } from "./store"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

/** The row's menu acts on the selected pattern; a right press selects the row first. */
const PATTERN_MENU: ContextItem[] = [
  "pattern.rename",
  "pattern.duplicate",
  contextSeparator,
  "pattern.delete",
  contextSeparator,
  "pattern.add",
]

/** "2 bars", or the steps when the pattern is not a whole number of bars. */
export function describePatternLength(
  lengthSteps: number,
  barTicks: number
): string {
  const bars = (lengthSteps * TICKS_PER_STEP) / barTicks
  if (Number.isInteger(bars)) return bars === 1 ? t("1 bar") : message("{bars} bars", { bars: plainNumber(bars) })
  return lengthSteps === 1 ? t("1 step") : message("{lengthSteps} steps", { lengthSteps: plainNumber(lengthSteps) })
}

/** Makes a pattern the brush, and the pattern selected everywhere else. */
function pickPattern(id: PatternId) {
  usePlaylistStore.getState().setBrush({ type: "pattern" })
  return setTransportPattern(id)
}

type PatternRowProps = {
  id: PatternId
  selected: boolean
  clips: number
  barTicks: number
}

const PatternRow = memo(function PatternRow({
  id,
  selected,
  clips,
  barTicks,
}: PatternRowProps) {
  const pattern = usePattern(id)
  const name = pattern?.name ?? ""
  const used =
    clips === 0
      ? t("not on the timeline yet")
      : clips === 1 ? t("on the timeline once") : message("on the timeline {clips} times", { clips: plainNumber(clips) })
  const hint = useHint(
    selected
      ? message("{name} is the pattern Draw and Paint place. It is {used}. Double-click to rename", { name, used })
      : message("Click to place {name} with Draw and Paint. It is {used}", { name, used })
  )
  if (!pattern) return null

  return (
    <ContextActions items={PATTERN_MENU}>
      <button
        type="button"
        aria-pressed={selected}
        data-pattern={id}
        onPointerDown={(event) => {
          // The menu's actions work on the selected pattern.
          if (event.button === 2 && !selected) void pickPattern(id)
        }}
        onClick={() => void pickPattern(id)}
        onDoubleClick={() => void runAction("pattern.rename")}
        className={cn(
          "group flex h-7 w-full shrink-0 items-center gap-2 border-l-2 border-transparent pr-2 pl-1.5 text-left outline-none hover:bg-accent/60 focus-visible:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
          selected && "border-brand bg-accent text-accent-foreground"
        )}
        {...hint}
      >
        <span
          aria-hidden
          className="h-3.5 w-2 shrink-0 rounded-[2px]"
          style={{ backgroundColor: colorToCss(pattern.color) }}
        />
        <span
          className={cn(
            "min-w-0 flex-1 truncate",
            selected ? "font-medium" : "text-foreground/85"
          )}
        >
          {pattern.name}
        </span>
        <span className="shrink-0 font-readout text-[0.625rem] text-muted-foreground">
          {describePatternLength(pattern.lengthSteps, barTicks)}
        </span>
      </button>
    </ContextActions>
  )
})

/**
 * What can be placed on the timeline, at its left: the project's patterns,
 * its sounds and its automations. The one that is picked is the brush:
 * Draw and Paint place it. Picking a pattern selects it everywhere, so the
 * channel rack shows it too.
 */
export function PatternPicker() {
  const [query, setQuery] = useState("")
  const ids = useProjectStore(
    useShallow((state) => matchingPatternIds(state.project.patterns, query))
  )
  const selectedPattern = useSelectedPatternId()
  const patternBrush = usePlaylistStore(
    (state) => state.brush.type === "pattern"
  )
  const selected = patternBrush ? selectedPattern : null
  const counts = useClipCounts(ids)
  const barTicks = ticksPerBar(useSettings().timeSignature)

  function onKeyDown(event: KeyboardEvent) {
    const step =
      event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0
    if (step === 0 || event.ctrlKey || event.metaKey || event.altKey) return
    // Kept from the playlist's own arrow keys, which move clips.
    event.preventDefault()
    void runAction(step === 1 ? "pattern.next" : "pattern.previous")
  }

  return (
    <aside
      aria-label={t("Clips to place")}
      className="flex min-h-0 shrink-0 flex-col overflow-y-auto border-r bg-chassis/30"
      style={{ width: PICKER_WIDTH }}
    >
      <PickerSection
        title={t("Patterns")}
        action={
          <ActionButton
            action="pattern.add"
            variant="ghost"
            size="icon-xs"
            tooltipSide="right"
          >
            <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
          </ActionButton>
        }
      >
        <div
          role="group"
          aria-label={t("Pattern to place")}
          onKeyDown={onKeyDown}
          className="flex flex-col py-0.5"
        >
          <label className="flex flex-col gap-1 px-2 py-1 text-[0.6875rem] text-muted-foreground">
            {t("Filter patterns")}
            <input
              type="text"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key.startsWith("Arrow")) event.stopPropagation()
              }}
              className="h-7 w-full min-w-0 rounded-md border border-input bg-input/20 px-2 text-xs text-foreground outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30"
            />
          </label>
          {ids.length === 0 && <PickerEmpty>{t("No patterns match.")}</PickerEmpty>}
          {ids.map((id, index) => (
            <PatternRow
              key={id}
              id={id}
              selected={id === selected}
              clips={counts[index] ?? 0}
              barTicks={barTicks}
            />
          ))}
        </div>
      </PickerSection>
      <AudioSection />
      <AutomationSection />
    </aside>
  )
}
