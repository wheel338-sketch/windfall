import { useState } from "react"

import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "@/components/ui/command"
import {
  disabledReason,
  isEnabled,
  runAction,
  shortcutLabel,
  useActions,
  useAppState,
  type Action,
} from "@/lib/actions"
import { useUiStore } from "@/lib/store/ui"
import { actionTitle, translatedText, useLanguageStore, useTranslation, type Language } from "@/lib/i18n"

import { scoreAction, shareOfWords } from "./score"

function bySection(actions: Action[]): [string, Action[]][] {
  const sections = new Map<string, Action[]>()
  for (const action of actions) {
    sections.set(action.section, [
      ...(sections.get(action.section) ?? []),
      action,
    ])
  }
  return [...sections]
}

/** The actions a search finds, best first: what the palette lists for it. */
export function searchActions(actions: readonly Action[], search: string, language: Language = useLanguageStore.getState().language) {
  return actions.map((action, index) => {
    const keywords = `${action.section} ${translatedText(action.section, language)} ${action.keywords ?? ""}`
    const score = Math.max(scoreAction(action.title, keywords, search), scoreAction(actionTitle(action.id, action.title, language), keywords, search))
    return { action, index, score }
  }).filter((entry) => entry.score > 0).sort((a, b) => b.score - a.score || a.index - b.index).map((entry) => entry.action)
}

/** How many actions a search nothing matches in full still suggests. */
const CLOSEST_LIMIT = 8

/**
 * For a search no action matches in full, such as a feature called by
 * another name: the actions that match at least half of its words, the
 * most first. Empty when nothing comes that close.
 */
export function closestActions(actions: readonly Action[], search: string, language: Language = useLanguageStore.getState().language) {
  return actions.map((action, index) => {
    const keywords = `${action.section} ${translatedText(action.section, language)} ${action.keywords ?? ""}`
    const share = Math.max(shareOfWords(action.title, keywords, search), shareOfWords(actionTitle(action.id, action.title, language), keywords, search))
    return { action, index, share }
  }).filter((entry) => entry.share >= 0.5).sort((a, b) => b.share - a.share || a.index - b.index).slice(0, CLOSEST_LIMIT).map((entry) => entry.action)
}

type ListProps = {
  search: string
  onRun(id: string): void
}

// Mounted only while the palette is open, so it can follow the app state.
function PaletteList({ search, onRun }: ListProps) {
  const t = useTranslation()
  const language = useLanguageStore((state) => state.language)
  const actions = useActions()
  const state = useAppState()
  const searching = search.trim() !== ""
  const found = searching ? searchActions(actions, search, language) : []
  const closest =
    searching && found.length === 0 ? closestActions(actions, search, language) : []

  const row = (action: Action, where?: string) => {
    const shortcut = shortcutLabel(action.id)
    const reason = disabledReason(action, state)
    return (
      <CommandItem
        key={action.id}
        value={action.id}
        disabled={!isEnabled(action, state)}
        onSelect={() => onRun(action.id)}
      >
        {actionTitle(action.id, action.title, language).replace(/…$/, "")}
        {where && (
          <span className="text-[0.625rem] text-muted-foreground">{t(where)}</span>
        )}
        {reason ? (
          <CommandShortcut className="tracking-normal">
            {t(reason)}
          </CommandShortcut>
        ) : (
          shortcut && <CommandShortcut>{shortcut}</CommandShortcut>
        )}
      </CommandItem>
    )
  }

  return (
    <CommandList className="max-h-[min(24rem,60vh)]">
      <CommandEmpty>{t("No action matches that.")}</CommandEmpty>
      {searching ? (
        // One list, best match first. Under their headings the best match
        // could sit below a whole section of worse ones.
        <>
          <CommandGroup>
            {found.map((action) => row(action, action.section))}
          </CommandGroup>
          {/* Words from elsewhere in the app can miss every title. Point at
              the nearest actions rather than at nothing. */}
          {closest.length > 0 && (
            <CommandGroup heading={t("Closest matches")}>
              {closest.map((action) => row(action, action.section))}
            </CommandGroup>
          )}
        </>
      ) : (
        bySection(actions).map(([section, items]) => (
          <CommandGroup key={section} heading={t(section)}>
            {items.map((action) => row(action))}
          </CommandGroup>
        ))
      )}
    </CommandList>
  )
}

/**
 * Every action in the registry in one searchable list, each with its
 * shortcut. Choosing one runs it.
 */
export function CommandPalette() {
  const t = useTranslation()
  const open = useUiStore((state) => state.dialog === "palette")
  const closeDialog = useUiStore((state) => state.closeDialog)
  const [search, setSearch] = useState("")
  // A palette opened again starts with an empty search.
  const [wasOpen, setWasOpen] = useState(open)
  if (open !== wasOpen) {
    setWasOpen(open)
    if (open) setSearch("")
  }

  function run(id: string) {
    closeDialog()
    void runAction(id)
  }

  return (
    <CommandDialog
      open={open}
      onOpenChange={(next) => {
        if (!next) closeDialog()
      }}
      title={t("Command palette")}
      description={t("Search for an action to run.")}
      className="sm:max-w-lg"
    >
      {/* cmdk's vim keys would swallow Ctrl+K, which closes the palette.
          The list is filtered and put in order here, not by cmdk: it sorts
          sections as wholes, and loses track of some of them. */}
      <Command vimBindings={false} shouldFilter={false}>
        <CommandInput
          placeholder={t("Search actions…")}
          value={search}
          onValueChange={setSearch}
        />
        <PaletteList search={search} onRun={run} />
      </Command>
    </CommandDialog>
  )
}
