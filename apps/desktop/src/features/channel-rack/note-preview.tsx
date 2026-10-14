import {
  memo,
  useEffect,
  useMemo,
  useRef,
  type KeyboardEvent,
  type ReactNode,
} from "react"

import type { ChannelId, Note, PatternId } from "@/bindings"
import { ActionMenuItem } from "@/components/action-menu-item"
import { ContextActions, type ContextItem } from "@/components/context-actions"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  disabledReason,
  getAppState,
  isChecked,
  isEnabled,
  registry,
  shortcutLabel,
  useAction,
  useActions,
  useAppState,
  useShortcutLabel,
} from "@/lib/actions"
import { useHint } from "@/lib/store/hint"
import { usePlayhead } from "@/lib/store/realtime"
import {
  getProjectGeneration,
  useProjectGeneration,
} from "@/lib/store/replaced"
import { useTransportStore } from "@/lib/store/transport"
import { TICKS_PER_STEP } from "@/lib/units"
import { cn } from "@/lib/utils"

import { NOTE_VIEW_ACTION_IDS, OPEN_NOTE_PREVIEW_ACTION } from "./actions"
import { pitches } from "./layout"
import {
  needsNotePreview,
  notePreviewGeometry,
  previewSteps,
} from "./note-preview-geometry"
import {
  notePreviewActionForTarget,
  notePreviewTargetReason,
  prepareNotePreviewTarget,
  runNotePreviewAction,
  type NotePreviewTarget,
} from "./note-preview-target"
import { rackNoteView, useRackStore } from "./rack-store"
import { actionTitle, translatedText as t, message, plainNumber } from "@/lib/i18n"

const ROW_ACTION_IDS = [...NOTE_VIEW_ACTION_IDS, OPEN_NOTE_PREVIEW_ACTION]

/** No async transport change: only the lane currently shown may navigate. */
function open(target: NotePreviewTarget) {
  const action = registry.get(OPEN_NOTE_PREVIEW_ACTION)
  if (action) void runNotePreviewAction(target, action)
}

function contextItems(target: NotePreviewTarget): ContextItem[] {
  const state = getAppState()
  return ROW_ACTION_IDS.flatMap((id) => {
    const action = registry.get(id)
    if (!action) return []
    const bound = notePreviewActionForTarget(action, target)
    return [
      {
        title: actionTitle(id, action.title),
        shortcut: shortcutLabel(id),
        disabled: !isEnabled(bound, state),
        reason: disabledReason(bound, state),
        checked: action.checked ? isChecked(bound, state) : undefined,
        afterClose: true,
        run: () => runNotePreviewAction(target, action),
      },
    ]
  })
}

function RowActionItems({ target }: { target: NotePreviewTarget }) {
  const actions = useActions()
  const state = useAppState()
  return (
    <DropdownMenuGroup
      onClickCapture={(event) => {
        if (
          !(event.target instanceof Element) ||
          !event.target.closest('[role^="menuitem"]')
        )
          return
        if (!prepareNotePreviewTarget(target)) {
          event.preventDefault()
          event.stopPropagation()
        }
      }}
    >
      <DropdownMenuLabel>{t("Row view")}</DropdownMenuLabel>
      {ROW_ACTION_IDS.map((id) => {
        const action = actions.find((item) => item.id === id)
        return (
          action && (
            <ActionMenuItem
              key={id}
              action={notePreviewActionForTarget(action, target)}
              state={state}
              inset
            />
          )
        )
      })}
    </DropdownMenuGroup>
  )
}

type PreviewProps = {
  target: NotePreviewTarget
  name: string
  notes: readonly Note[] | undefined
  lengthSteps: number
  color: string
  quiet: boolean
}

const NotePreview = memo(function NotePreview({
  target,
  name,
  notes,
  lengthSteps,
  color,
  quiet,
}: PreviewProps) {
  const openAction = useAction(OPEN_NOTE_PREVIEW_ACTION)
  const openShortcut = useShortcutLabel(OPEN_NOTE_PREVIEW_ACTION)
  const geometry = useMemo(
    () => notePreviewGeometry(notes, lengthSteps),
    [notes, lengthSteps]
  )
  const pressed = useRef<NotePreviewTarget | null>(null)
  const caret = useRef<HTMLSpanElement>(null)
  const mounted = useRef(false)
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])
  const hint = useHint(
    t("Note preview in rack. Click or press Enter to open this channel in the piano roll. Space plays or stops. The row view menu offers step buttons")
  )
  // Cursor style writes per frame, independent of the number of notes; no React.
  usePlayhead((tick, playing) => {
    const mark = caret.current
    if (!mark) return
    const transport = useTransportStore.getState()
    const inside =
      Number.isFinite(tick) &&
      tick >= 0 &&
      tick < previewSteps(lengthSteps) * TICKS_PER_STEP
    const running =
      playing &&
      transport.playing &&
      transport.mode === "pattern" &&
      transport.pattern === target.pattern &&
      target.generation === getProjectGeneration() &&
      inside
    mark.style.visibility = running ? "visible" : "hidden"
    if (running) mark.style.left = pitches(tick / TICKS_PER_STEP)
  })

  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (
      event.altKey ||
      event.ctrlKey ||
      event.metaKey ||
      event.shiftKey ||
      event.nativeEvent.isComposing
    )
      return
    if (event.key === "Enter") {
      event.preventDefault()
      event.stopPropagation()
      if (!event.repeat && mounted.current) open(target)
      return
    }
    let row = event.currentTarget.closest<HTMLElement>("[data-channel-row]")
    let next: HTMLElement | null
    if (event.key === "ArrowLeft")
      next = row?.querySelector("[data-channel-button]") ?? null
    else if (event.key === "ArrowUp" || event.key === "ArrowDown") {
      const neighbor =
        event.key === "ArrowUp"
          ? row?.previousElementSibling
          : row?.nextElementSibling
      row = neighbor instanceof HTMLElement ? neighbor : null
      next =
        row?.querySelector('[data-slot="step-grid"] button[tabindex="0"]') ??
        null
    } else return
    event.preventDefault()
    event.stopPropagation()
    next?.focus({ preventScroll: true })
  }

  const description = [
    message("{shown} notes in the pattern", { count: geometry.count, shown: plainNumber(geometry.count) }),
    geometry.outside ? message("{outside} outside the preview", { outside: plainNumber(geometry.outside) }) : "",
    geometry.dense ? t("dense notes are combined in the thumbnail") : "",
  ].filter(Boolean).join("; ")
  const openTitle = openAction ? actionTitle(openAction.id, openAction.title) : ""
  return (
    // Keep the existing channel-name arrow-navigation hook, without step buttons.
    <div
      data-slot="step-grid"
      className="h-5.5 shrink-0"
      style={{ width: pitches(previewSteps(lengthSteps)) }}
    >
      <ContextActions items={() => contextItems(target)}>
        <Button
          variant="ghost"
          className={cn(
            "relative h-full w-full overflow-hidden rounded-[3px] border-0 p-0",
            quiet && "opacity-40"
          )}
          style={{ backgroundColor: "var(--wf-step-off)" }}
          tabIndex={0}
          data-slot="rack-note-preview"
          aria-label={message("{name} note preview. {openActionTitle}", { name, openActionTitle: openTitle })}
          title={message("{description}. {action}", { description, action: openShortcut ? message("{help} ({shortcut})", { help: openTitle, shortcut: openShortcut }) : openTitle })}
          onKeyDown={onKeyDown}
          onPointerDown={(event) => {
            pressed.current = event.button === 0 ? target : null
          }}
          onPointerCancel={() => {
            pressed.current = null
          }}
          onClick={(event) => {
            const captured = event.detail > 0 ? pressed.current : target
            pressed.current = null
            if (mounted.current && captured) open(captured)
          }}
          {...hint}
        >
          <svg
            aria-hidden
            viewBox={`0 0 ${geometry.width} ${geometry.height}`}
            preserveAspectRatio="none"
            style={{ width: "100%", height: "100%" }}
          >
            <path
              data-note-shapes
              d={geometry.path}
              fill={color}
              fillRule="nonzero"
            />
          </svg>
          <span className="sr-only">{description}</span>
          <span
            ref={caret}
            aria-hidden
            data-slot="rack-note-playhead"
            className="pointer-events-none absolute inset-y-0 w-px bg-(--wf-playhead)"
            style={{ visibility: "hidden" }}
          />
          {geometry.count === 0 && (
            <span className="absolute inset-0 flex items-center justify-center text-muted-foreground">
              {t("No notes in this pattern")}
            </span>
          )}
        </Button>
      </ContextActions>
    </div>
  )
})

/** The view choice belongs to this document/lane, not to its notes. */
export function RackNoteArea({
  pattern,
  channel,
  name,
  notes,
  lengthSteps,
  color,
  quiet,
  children,
}: Omit<PreviewProps, "target"> & {
  pattern: PatternId
  channel: ChannelId
  children: ReactNode
}) {
  const generation = useProjectGeneration()
  const key = `${pattern}:${channel}`
  const choice = useRackStore((state) =>
    rackNoteView(state, { pattern, channel })
  )
  const automatic = useMemo(() => needsNotePreview(notes), [notes])
  const showNotes = choice === "notes" || (choice === "auto" && automatic)
  const target = useMemo(
    () => ({ pattern, channel, generation }),
    [pattern, channel, generation]
  )
  const focus = useRef<HTMLDivElement>(null)

  // Menu finalFocus follows the new control when changing view unmounts the old.
  return (
    <>
      <div ref={focus} className="contents">
        {showNotes ? (
          <NotePreview
            key={`${generation}:${key}`}
            target={target}
            name={name}
            notes={notes}
            lengthSteps={lengthSteps}
            color={color}
            quiet={quiet}
          />
        ) : (
          children
        )}
      </div>
      <DropdownMenu key={`${generation}:${key}`}>
        <DropdownMenuTrigger
          render={
            <Button
              variant="ghost"
              size="icon-xs"
              className="sticky right-0 z-10 -mr-3 h-5.5 w-3 rounded-none bg-background"
              aria-label={message("{name} row view", { name })}
            />
          }
        >
          <span aria-hidden>⋮</span>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          finalFocus={() =>
            notePreviewTargetReason(target)
              ? false
              : (focus.current?.querySelector<HTMLElement>(
                  '[data-slot="step-grid"] button[tabindex="0"]'
                ) ?? false)
          }
        >
          <RowActionItems target={target} />
        </DropdownMenuContent>
      </DropdownMenu>
    </>
  )
}
