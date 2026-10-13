import { Moon02Icon, Search01Icon, Sun03Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import { ActionButton } from "@/components/action-button"
import { ContextActions } from "@/components/context-actions"
import { Kbd } from "@/components/ui/kbd"
import { runAction, useShortcutLabel } from "@/lib/actions"
import { useHint } from "@/lib/store/hint"
import { useDirty, useProjectName } from "@/lib/store/selectors"
import { resolveTheme, useUiStore } from "@/lib/store/ui"
import { useTranslation, translatedText as t } from "@/lib/i18n"

import { TITLE_MENU } from "./chrome-menus"
import { AppMenuBar } from "./menu-bar"

function Mark() {
  return (
    <span
      aria-hidden
      className="flex size-[1.125rem] shrink-0 items-center justify-center rounded-[5px] bg-brand text-[0.6875rem] font-semibold text-brand-foreground"
    >
      W
    </span>
  )
}

function ProjectTitle() {
  const name = useProjectName()
  const dirty = useDirty()
  return (
    // It has the room between the menus and the search box, and gives way
    // to both: in a narrow window the name is cut short, not laid over them.
    <div
      data-slot="project-title"
      className="flex min-w-0 flex-1 justify-center"
    >
      <span className="flex max-w-full min-w-0 items-center gap-1.5 text-foreground/70">
        <span className="truncate">{name}</span>
        {dirty && (
          <span
            role="img"
            aria-label={t("Unsaved changes")}
            className="size-1.5 shrink-0 rounded-full bg-brand"
          />
        )}
      </span>
    </div>
  )
}

function PaletteButton() {
  const t = useTranslation()
  const shortcut = useShortcutLabel("view.commandPalette")
  const hint = useHint(t("Find any action by name and see its shortcut"))
  return (
    <button
      type="button"
      onClick={() => void runAction("view.commandPalette")}
      className="flex h-6 w-52 items-center gap-1.5 rounded-md border border-input/70 bg-background/60 pr-1 pl-2 text-muted-foreground outline-none hover:border-input hover:text-foreground focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30"
      {...hint}
    >
      <HugeiconsIcon icon={Search01Icon} strokeWidth={2} className="size-3" />
      <span className="flex-1 text-left">{t("Search actions")}</span>
      {shortcut && <Kbd className="h-4 min-w-4">{shortcut}</Kbd>}
    </button>
  )
}

function ThemeButton() {
  const dark = useUiStore((state) => resolveTheme(state.theme) === "dark")
  return (
    <ActionButton action="view.toggleTheme" variant="ghost" size="icon-sm">
      <HugeiconsIcon icon={dark ? Sun03Icon : Moon02Icon} strokeWidth={2} />
    </ActionButton>
  )
}

/** The top strip: menus on the left, the project name, and search on the right. */
export function TitleBar() {
  return (
    <ContextActions items={TITLE_MENU}>
      <header className="flex h-8 shrink-0 items-center gap-2 border-b bg-chassis pr-1.5 pl-2">
        <Mark />
        <div className="shrink-0">
          <AppMenuBar />
        </div>
        <ProjectTitle />
        <div className="flex shrink-0 items-center gap-1">
          <PaletteButton />
          <ThemeButton />
        </div>
      </header>
    </ContextActions>
  )
}
