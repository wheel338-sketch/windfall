import { useEffect, useRef, useSyncExternalStore } from "react"

import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { onHistoryNavigation } from "@/lib/store/project"
import { useSession } from "./context"
import type { PendingStampChoice } from "./editor"
import { CHORD_STAMPS, SCALE_STAMPS, type Stamp } from "./stamps"
import { usePianoRollStore } from "./store"
import { translatedText as t, message } from "@/lib/i18n"

export function StampMenu() {
  const session = useSession()
  const { editor } = session
  const pending = useRef<PendingStampChoice | null>(null)
  const state = useSyncExternalStore(
    (listener) => editor.subscribe(listener),
    () => editor.stampState,
    () => null
  )
  const choose = (stamp: Stamp) => {
    pending.current = editor.deferStamp(stamp)
  }

  useEffect(() => {
    const cancelPending = () => pending.current?.cancel()
    const onKeyDown = (event: KeyboardEvent) => {
      // The closing menu may still own focus, so the grid keymap can ignore Esc.
      if (event.key === "Escape") cancelPending()
    }
    const stopSettings = usePianoRollStore.subscribe((next, previous) => {
      if (
        next.tool !== previous.tool ||
        next.scaleRoot !== previous.scaleRoot ||
        next.scaleId !== previous.scaleId ||
        next.highlightScale !== previous.highlightScale ||
        next.snapToScale !== previous.snapToScale
      )
        cancelPending()
    })
    // History intent revokes this lease before IPC, including edits outside
    // the lane. Returning to the same cursor must not restore its ownership.
    const stopHistory = onHistoryNavigation(cancelPending)
    window.addEventListener("blur", cancelPending)
    window.addEventListener("keydown", onKeyDown)
    return () => {
      cancelPending()
      stopSettings()
      stopHistory()
      window.removeEventListener("blur", cancelPending)
      window.removeEventListener("keydown", onKeyDown)
    }
  }, [editor])

  return (
    <>
      <DropdownMenu
        onOpenChangeComplete={(open) => {
          if (open) return
          const chosen = pending.current
          pending.current = null
          // Closing menus manage focus until their exit transition completes.
          if (chosen?.complete()) session.focusGrid()
        }}
      >
        <DropdownMenuTrigger
          render={
            <Button
              variant={state ? "secondary" : "outline"}
              size="sm"
              aria-label={t("Choose chord or scale stamp")}
            />
          }
        >
          {t("Stamp")}
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-80">
          <DropdownMenuGroup>
            <DropdownMenuLabel>
              {t("Choose a pattern, then click its root in the grid")}
            </DropdownMenuLabel>
            <DropdownMenuSub>
              <DropdownMenuSubTrigger>{t("Chords")}</DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="w-64">
                <DropdownMenuGroup>
                  <DropdownMenuLabel>{t("Simultaneous notes")}</DropdownMenuLabel>
                  {CHORD_STAMPS.map((stamp) => (
                    <DropdownMenuItem
                      key={stamp.id}
                      onClick={() => choose(stamp)}
                    >
                      {stamp.label}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuGroup>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
            {(["ascending", "descending"] as const).map((direction) => (
              <DropdownMenuSub key={direction}>
                <DropdownMenuSubTrigger>
                  {direction === "ascending"
                    ? t("Ascending scales")
                    : t("Descending scales")}
                </DropdownMenuSubTrigger>
                <DropdownMenuSubContent className="w-72">
                  <DropdownMenuGroup>
                    <DropdownMenuLabel>
                      {t("One note length per scale step")}
                    </DropdownMenuLabel>
                    {SCALE_STAMPS.filter(
                      (stamp) => stamp.layout === direction
                    ).map((stamp) => (
                      <DropdownMenuItem
                        key={stamp.id}
                        onClick={() => choose(stamp)}
                      >
                        {stamp.label}
                      </DropdownMenuItem>
                    ))}
                  </DropdownMenuGroup>
                </DropdownMenuSubContent>
              </DropdownMenuSub>
            ))}
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>
      {state && (
        <>
          <Button
            variant="ghost"
            size="sm"
            aria-label={t("Cancel stamp placement")}
            onClick={() => {
              editor.cancel()
              session.focusGrid()
            }}
          >
            {t("Cancel")}
          </Button>
          <span
            role="status"
            aria-label={t("Stamp placement")}
            className="max-w-80 shrink-0 truncate text-muted-foreground"
            title={state.error ?? undefined}
          >
            {state.error ??
              message("{stampLabel}: click to place · Esc cancels", { stampLabel: state.stamp.label })}
          </span>
        </>
      )}
    </>
  )
}
