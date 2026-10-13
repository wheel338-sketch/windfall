import { Clock04Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { useId, useRef, useState } from "react"

import { Button } from "@/components/ui/button"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover"
import { useHint } from "@/lib/store/hint"
import { useHistory } from "@/lib/store/selectors"
import { useUiStore } from "@/lib/store/ui"

import { HistoryList } from "./history-list"
import { translatedText as t } from "@/lib/i18n"

/** Mounted only while open, so closing discards the unsaved filter. */
function FilteredHistory() {
  const [query, setQuery] = useState("")
  const id = useId()

  return (
    <>
      <FieldGroup>
        <Field>
          <FieldLabel htmlFor={id}>{t("Filter history")}</FieldLabel>
          <Input
            id={id}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </Field>
      </FieldGroup>
      <HistoryList query={query} className="max-h-72" />
    </>
  )
}

/** A button that opens the undo history. Edit > History opens it too. */
export function HistoryPopover() {
  const { entries } = useHistory()
  const open = useUiStore((state) => state.historyOpen)
  const setOpen = useUiStore((state) => state.setHistoryOpen)
  const content = useRef<HTMLDivElement>(null)
  const hint = useHint(
    t("History: every edit, in order. Click one to go back to it")
  )

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Button
            variant="ghost"
            size="icon"
            aria-label={t("Undo history")}
            {...hint}
          />
        }
      >
        <HugeiconsIcon icon={Clock04Icon} strokeWidth={2} />
      </PopoverTrigger>
      <PopoverContent
        ref={content}
        align="end"
        className="w-64 gap-2 p-2"
        // The focus goes to the current step, not to the top of the list,
        // which would scroll a long history back to its start.
        initialFocus={() =>
          content.current?.querySelector<HTMLElement>(
            "[aria-current='step']"
          ) ?? true
        }
      >
        <PopoverHeader className="px-1">
          <PopoverTitle className="text-xs">{t("History")}</PopoverTitle>
          <PopoverDescription>
            {entries.length === 0
              ? t("Nothing to undo yet. Edits will be listed here.")
              : t("Click a step to go back or forward to it.")}
          </PopoverDescription>
        </PopoverHeader>
        {open && <FilteredHistory />}
      </PopoverContent>
    </Popover>
  )
}
