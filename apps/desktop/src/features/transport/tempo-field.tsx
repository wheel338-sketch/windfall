import { logicalDelta } from "@/lib/ui-scale"
import {
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
} from "react"

import { ValueInput } from "@/components/audio"
import { ContextActions, contextSeparator } from "@/components/context-actions"
import { valueMenuItems } from "@/components/value-context-menu"
import { automatedNow, useAutomationMarker } from "@/features/automation/live"
import { automationItems } from "@/features/automation/menu"
import { cn } from "@/lib/utils"
import { useGesture } from "@/lib/store/gesture"
import { useHint } from "@/lib/store/hint"
import { dispatch } from "@/lib/store/project"
import { useRealtime } from "@/lib/store/realtime"
import { useTempo } from "@/lib/store/selectors"
import { formatTempo, normalizeTempo, parseTempo } from "@/lib/time"
import { DEFAULT_TEMPO_BPM, MAX_TEMPO_BPM, MIN_TEMPO_BPM } from "@/lib/units"

import { nextTempoPreset } from "./tempo-preset-step"
import { nextTempo, TEMPO_PRESETS } from "./tempo-presets"
import { translatedText as t, dynamicTitle, message } from "@/lib/i18n"

/** How far the pointer must move before a press counts as a drag. */
const DRAG_THRESHOLD_PX = 3
const PIXELS_PER_BPM = 3

const TEMPO_TARGET = { type: "tempo" } as const
const TEMPO_AUTOMATION = automationItems(TEMPO_TARGET)

type Drag = {
  startY: number
  startTempo: number
  moved: boolean
  sent: number
  pending: Promise<unknown>
}

/** The text entry: what it opens with, and whether typing replaces that. */
type Draft = { text: string; selectAll: boolean }

/**
 * The tempo. Drag it up or down, nudge it with the arrow keys, or click it
 * and type. A drag shows its own value while it lasts and sends every change
 * under one gesture, so the whole drag is a single undo step.
 *
 * The text entry is the kit's: Enter and Tab set the tempo, and Escape or
 * leaving the field any other way changes nothing.
 *
 * While the song plays through a tempo automation the readout follows the
 * curve, in the automation's color. What is edited is still the stored
 * tempo, which is what the song has wherever no curve reaches it.
 */
export function TempoField() {
  const tempo = useTempo()
  const gesture = useGesture()
  const drag = useRef<Drag | null>(null)
  const [dragValue, setDragValue] = useState<number | null>(null)
  const [draft, setDraft] = useState<Draft | null>(null)
  const root = useRef<HTMLDivElement>(null)
  // The field takes the keyboard back when its entry is closed by a key.
  const refocus = useRef(false)
  useLayoutEffect(() => {
    if (draft === null && refocus.current) {
      refocus.current = false
      root.current?.focus({ preventScroll: true })
    }
  }, [draft])
  const liveText = useRef<HTMLSpanElement>(null)
  const marker = useAutomationMarker(TEMPO_TARGET)
  // Written straight to the page: the tempo can change every frame.
  useRealtime(() => {
    const field = root.current
    if (!field) return
    const now = automatedNow(TEMPO_TARGET)
    if (!now) {
      if (field.hasAttribute("data-live")) field.removeAttribute("data-live")
      return
    }
    const text = formatTempo(now.value)
    if (liveText.current && liveText.current.textContent !== text) {
      liveText.current.textContent = text
    }
    if (!field.hasAttribute("data-live")) {
      field.setAttribute("data-live", "")
      field.style.setProperty("--live-color", now.color)
    }
  })
  const hint = useHint(
    t("Tempo: drag up or down, hold Shift for fine steps, or click to type")
  )

  const shown = dragValue ?? tempo

  function setTempo(value: number) {
    const tempoBpm = normalizeTempo(value)
    if (tempoBpm !== tempo) {
      void dispatch({ type: "updateSettings", patch: { tempoBpm } })
    }
  }

  function onPointerDown(event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return
    event.currentTarget.setPointerCapture(event.pointerId)
    drag.current = {
      startY: event.clientY,
      startTempo: tempo,
      moved: false,
      sent: tempo,
      pending: Promise.resolve(),
    }
  }

  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    const current = drag.current
    if (!current) return
    const distance = logicalDelta(current.startY - event.clientY)
    if (!current.moved) {
      if (Math.abs(distance) < DRAG_THRESHOLD_PX) return
      current.moved = true
      gesture.begin()
    }
    const next = event.shiftKey
      ? normalizeTempo(current.startTempo + distance * 0.01)
      : normalizeTempo(
          Math.round(current.startTempo + distance / PIXELS_PER_BPM)
        )
    setDragValue(next)
    if (next === current.sent) return
    current.sent = next
    current.pending = gesture.dispatch({
      type: "updateSettings",
      patch: { tempoBpm: next },
    })
  }

  function onPointerUp(event: PointerEvent<HTMLDivElement>) {
    const current = drag.current
    drag.current = null
    if (!current) return
    event.currentTarget.releasePointerCapture(event.pointerId)
    if (!current.moved) {
      startEditing()
      return
    }
    gesture.end()
    // Hold the dragged value until the store has caught up, so the number
    // does not jump back for a frame.
    void current.pending.finally(() => setDragValue(null))
  }

  function startEditing() {
    setDraft({ text: formatTempo(tempo), selectAll: true })
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    // A key with Ctrl, Alt or Cmd is a shortcut, never a digit of a tempo:
    // Alt+2 shows the playlist and must not start typing "2".
    if (event.altKey || event.ctrlKey || event.metaKey) return
    const step = event.shiftKey ? 0.1 : 1
    if (event.key === "ArrowUp") setTempo(tempo + step)
    else if (event.key === "ArrowDown") setTempo(tempo - step)
    else if (event.key === "PageUp") setTempo(tempo + 10)
    else if (event.key === "PageDown") setTempo(tempo - 10)
    else if (event.key === "Enter") startEditing()
    else if (/^[0-9]$/.test(event.key)) {
      setDraft({ text: event.key, selectAll: false })
    } else return
    event.preventDefault()
    event.stopPropagation()
  }

  function closeEntry(byKey: boolean) {
    refocus.current = byKey
    setDraft(null)
  }

  if (draft !== null) {
    return (
      <ValueInput
        aria-label={t("Tempo in beats per minute")}
        initialText={draft.text}
        selectAll={draft.selectAll}
        onCommit={(text) => {
          closeEntry(true)
          const value = parseTempo(text)
          if (value !== null) setTempo(value)
        }}
        onCancel={(reason) => closeEntry(reason === "escape")}
        className="h-6 w-[4.25rem] border-0 bg-display-foreground/10 px-1 text-right font-readout text-[0.8125rem] font-medium text-display-foreground ring-1 ring-brand"
      />
    )
  }

  // The tempo is not one of the kit's controls, so it makes the menu every
  // value control has for itself, with the tempo's own entries first.
  const menu = () =>
    valueMenuItems(
      {
        value: tempo,
        text: `${formatTempo(tempo)} BPM`,
        defaultValue: DEFAULT_TEMPO_BPM,
        disabled: false,
        editable: true,
        unitKind: "bpm",
        reset: () => setTempo(DEFAULT_TEMPO_BPM),
        startEditing,
        change: setTempo,
        parse: (text) => parseTempo(text.replace(/\s*bpm$/i, "")),
      },
      t("Tempo"),
      [
        ...TEMPO_AUTOMATION,
        contextSeparator,
        {
          submenu: t("Tempo presets"),
          items: TEMPO_PRESETS.map((preset) => ({
            title: `${preset} BPM`,
            disabled: nextTempo(tempo, preset) === null,
            run: () => setTempo(preset),
          })),
        },
        {
          ...dynamicTitle("Previous tempo preset", {}),
          disabled: nextTempoPreset(tempo, "previous") === null,
          run: () => {
            const next = nextTempoPreset(tempo, "previous")
            if (next !== null) setTempo(next)
          },
        },
        {
          ...dynamicTitle("Next tempo preset", {}),
          disabled: nextTempoPreset(tempo, "next") === null,
          run: () => {
            const next = nextTempoPreset(tempo, "next")
            if (next !== null) setTempo(next)
          },
        },
        "tempo.tap",
        "tempo.half",
        "tempo.double",
      ]
    )

  return (
    <ContextActions items={menu}>
      <div
        ref={root}
        role="spinbutton"
        tabIndex={0}
        aria-label={t("Tempo in beats per minute")}
        aria-valuenow={shown}
        aria-valuemin={MIN_TEMPO_BPM}
        aria-valuemax={MAX_TEMPO_BPM}
        aria-valuetext={message("{tempo} beats per minute", { tempo: formatTempo(shown) })}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onKeyDown={onKeyDown}
        {...hint}
        className={cn(
          "group/tempo relative flex h-6 w-[4.25rem] cursor-ns-resize touch-none items-center justify-end rounded-sm px-1 font-readout text-[0.8125rem] font-medium text-display-foreground outline-none hover:bg-display-foreground/10 focus-visible:ring-1 focus-visible:ring-brand",
          dragValue !== null && "bg-display-foreground/10"
        )}
      >
        {marker !== undefined && (
          <span
            data-slot="tempo-marker"
            aria-hidden
            className="pointer-events-none absolute top-0.5 left-0.5 size-[5px] rounded-full"
            style={{ backgroundColor: marker }}
          />
        )}
        <span className="group-data-live/tempo:hidden">
          {formatTempo(shown)}
        </span>
        <span
          ref={liveText}
          data-slot="tempo-live"
          className="hidden text-(--live-color) group-data-live/tempo:inline"
        />
      </div>
    </ContextActions>
  )
}
