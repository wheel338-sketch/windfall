import { ArrowDown01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import type { PatternId } from "@/bindings"
import { ActionMenuItem } from "@/components/action-menu-item"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { useActions, useAppState } from "@/lib/actions"
import { useHint } from "@/lib/store/hint"
import {
  usePattern,
  usePatternIds,
  useSelectedPatternId,
} from "@/lib/store/selectors"
import { setTransportPattern } from "@/lib/store/transport"
import { colorToCss } from "@/lib/units"
import { translatedText as t, message } from "@/lib/i18n"

const PATTERN_ACTIONS = [
  "pattern.add",
  "pattern.duplicate",
  "pattern.rename",
  "pattern.delete",
]

function PatternSwatch({ color }: { color: number }) {
  return (
    <span
      aria-hidden
      className="size-2 shrink-0 rounded-[2px]"
      style={{ backgroundColor: colorToCss(color) }}
    />
  )
}

function PatternOption({ id }: { id: PatternId }) {
  const pattern = usePattern(id)
  if (!pattern) return null
  return (
    <DropdownMenuRadioItem value={id}>
      <PatternSwatch color={pattern.color} />
      <span className="truncate">{pattern.name}</span>
    </DropdownMenuRadioItem>
  )
}

function PatternMenu() {
  const ids = usePatternIds()
  const selected = useSelectedPatternId()
  const actions = useActions()
  const state = useAppState()

  return (
    <>
      <DropdownMenuGroup>
        <DropdownMenuLabel>{t("Patterns")}</DropdownMenuLabel>
        <DropdownMenuRadioGroup
          value={selected}
          onValueChange={(id: PatternId) => void setTransportPattern(id)}
        >
          {ids.map((id) => (
            <PatternOption key={id} id={id} />
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuGroup>
      <DropdownMenuSeparator />
      {PATTERN_ACTIONS.map((id) => {
        const action = actions.find((item) => item.id === id)
        return (
          action && <ActionMenuItem key={id} action={action} state={state} />
        )
      })}
    </>
  )
}

/** Chooses the pattern being edited, and adds, renames, copies or deletes one. */
export function PatternSelector() {
  const pattern = usePattern(useSelectedPatternId())
  const hint = useHint(t("Pattern: choose which pattern to edit and play"))

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant="outline"
            aria-label={message("Pattern: {patternName}", { patternName: pattern?.name ?? t("none") })}
            className="w-40 justify-start gap-2 px-2"
            {...hint}
          />
        }
      >
        {pattern && <PatternSwatch color={pattern.color} />}
        <span className="min-w-0 flex-1 truncate text-left">
          {pattern?.name ?? t("No pattern")}
        </span>
        <HugeiconsIcon
          icon={ArrowDown01Icon}
          strokeWidth={2}
          className="text-muted-foreground"
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent className="w-56">
        <PatternMenu />
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
