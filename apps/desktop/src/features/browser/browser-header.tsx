import {
  Cancel01Icon,
  FolderAddIcon,
  Search01Icon,
  StopIcon,
  VolumeHighIcon,
  VolumeOffIcon,
} from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { useEffect, useRef } from "react"

import { ActionButton } from "@/components/action-button"
import { ToggleLed } from "@/components/audio"
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from "@/components/ui/input-group"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { runAction } from "@/lib/actions"
import { useHint } from "@/lib/store/hint"

import {
  requestDone,
  requestTreeFocus,
  setFilter,
  useBrowserStore,
} from "./store"
import { translatedText as t } from "@/lib/i18n"

function FilterBox() {
  const filter = useBrowserStore((state) => state.filter)
  const focusRequest = useBrowserStore((state) => state.focusFilter)
  const inputRef = useRef<HTMLInputElement>(null)
  const hint = useHint(
    t("Search filenames and paths in every configured folder. Use * and ?, AND, OR, NOT, quotes and parentheses.")
  )

  // A request made while the panel was hidden is answered when it shows.
  useEffect(() => {
    if (!focusRequest) return
    inputRef.current?.focus()
    inputRef.current?.select()
    requestDone("focusFilter")
  }, [focusRequest])

  function onKeyDown(event: React.KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Escape") {
      event.preventDefault()
      if (filter !== "") setFilter("")
      else requestTreeFocus()
    } else if (event.key === "ArrowDown" || event.key === "Enter") {
      // On to the results, where the arrow keys audition them.
      event.preventDefault()
      requestTreeFocus()
    }
  }

  return (
    <InputGroup className="h-6 min-w-0 flex-1">
      <InputGroupAddon className="pl-1.5">
        <HugeiconsIcon icon={Search01Icon} strokeWidth={2} className="size-3" />
      </InputGroupAddon>
      <InputGroupInput
        ref={inputRef}
        type="text"
        role="searchbox"
        aria-label={t("Filter the browser")}
        placeholder={t("Search library")}
        maxLength={512}
        spellCheck={false}
        autoComplete="off"
        value={filter}
        onChange={(event) => setFilter(event.target.value)}
        onKeyDown={onKeyDown}
        className="h-6 px-1"
        {...hint}
      />
      {filter !== "" && (
        <InputGroupAddon align="inline-end" className="pr-1">
          <InputGroupButton
            size="icon-xs"
            aria-label={t("Clear the filter")}
            className="size-4"
            onClick={() => {
              setFilter("")
              inputRef.current?.focus()
            }}
          >
            <HugeiconsIcon icon={Cancel01Icon} strokeWidth={2} />
          </InputGroupButton>
        </InputGroupAddon>
      )}
    </InputGroup>
  )
}

function AutoPreviewToggle() {
  const autoPreview = useBrowserStore((state) => state.autoPreview)
  const label = t("Preview sounds when selected")
  const hint = useHint(
    autoPreview
      ? t("Selecting a sound plays it. Click to turn that off.")
      : t("Selecting a sound is silent. Click to play sounds as you select them.")
  )

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <ToggleLed
            size="lg"
            pressed={autoPreview}
            aria-label={label}
            onPressedChange={() => void runAction("browser.toggleAutoPreview")}
            {...hint}
          />
        }
      >
        <HugeiconsIcon
          icon={autoPreview ? VolumeHighIcon : VolumeOffIcon}
          strokeWidth={2}
          className="size-3.5"
        />
      </TooltipTrigger>
      <TooltipContent side="bottom">{label}</TooltipContent>
    </Tooltip>
  )
}

/** The strip above the tree: filter, auto-preview, stop, add folder. */
export function BrowserHeader() {
  return (
    <div className="flex h-8 shrink-0 items-center gap-1 border-b px-1.5">
      <FilterBox />
      <AutoPreviewToggle />
      <ActionButton
        action="browser.stopPreview"
        variant="ghost"
        size="icon-sm"
        className="text-muted-foreground"
      >
        <HugeiconsIcon
          icon={StopIcon}
          strokeWidth={2}
          className="fill-current"
        />
      </ActionButton>
      <ActionButton
        action="browser.addFolder"
        variant="ghost"
        size="icon-sm"
        className="text-muted-foreground"
      >
        <HugeiconsIcon icon={FolderAddIcon} strokeWidth={2} />
      </ActionButton>
    </div>
  )
}
