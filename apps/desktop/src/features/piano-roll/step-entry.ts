import { create } from "zustand"

import type { ChannelId } from "@/bindings"
import { actionForEvent } from "@/lib/actions"
import { shortcutAllowed } from "@/lib/actions/keymap"
import { dispatch } from "@/lib/store/project"
import { realtimeFrame } from "@/lib/store/realtime"
import { onProjectReplaced } from "@/lib/store/replaced"
import { useTransportStore } from "@/lib/store/transport"
import {
  DEFAULT_VELOCITY,
  MAX_PATTERN_TICKS,
  TICKS_PER_STEP,
} from "@/lib/units"

import { auditionOff, auditionOn } from "./audition"
import { patternMeterAt } from "./pattern-timeline"
import { currentSession } from "./session"
import { snapTicks } from "./snap"
import { liveInputSnap } from "./store"
import { NOTE_OFFSETS, useTypingKeyboardStore } from "./typing-keyboard"

type StepEntryState = {
  enabled: boolean
  cursor: number | null
  setEnabled(enabled: boolean): void
}

/** Session-only insertion position; enabling starts at the transport position. */
export const useStepEntryStore = create<StepEntryState>((set) => ({
  enabled: false,
  cursor: null,
  setEnabled: (enabled) =>
    set({
      enabled,
      cursor: enabled ? Math.max(0, Math.round(realtimeFrame().tick)) : null,
    }),
}))

onProjectReplaced(() =>
  useStepEntryStore.setState({ enabled: false, cursor: null })
)

/** Runs before Typing so the two modes share pitches without double auditioning. */
export function attachStepEntry(
  root: HTMLElement,
  channel: ChannelId
): () => void {
  const held = new Map<string, number>()
  const identity = (event: KeyboardEvent) =>
    event.code || event.key.toLowerCase()
  const release = () => {
    for (const key of held.values()) auditionOff(channel, key)
    held.clear()
  }
  const onKeyDown = (event: KeyboardEvent) => {
    const state = useStepEntryStore.getState()
    if (!state.enabled || event.defaultPrevented || event.isComposing) return
    if (
      !root.contains(document.activeElement) ||
      !shortcutAllowed(event.target, "A")
    )
      return
    const action = actionForEvent(event)
    if (
      action?.id === "pianoRoll.typing" ||
      action?.id === "pianoRoll.stepEntry"
    )
      return
    if (event.ctrlKey || event.metaKey || event.altKey) return
    const letter = event.key.toLowerCase()
    const offset = NOTE_OFFSETS[letter]
    const octaveKey = letter === "z" || letter === "x"
    const toolKey =
      action?.id.startsWith("pianoRoll.tool") && /^[a-z]$/.test(letter)
    if (typeof offset !== "number" && !octaveKey && !toolKey) return

    event.preventDefault()
    event.stopImmediatePropagation()
    if (event.repeat) return
    const typing = useTypingKeyboardStore.getState()
    if (octaveKey) {
      typing.shiftOctave(letter === "z" ? -1 : 1)
      return
    }
    const id = identity(event)
    if (typeof offset !== "number" || held.has(id)) return
    const key = typing.baseKey + offset
    held.set(id, key)
    auditionOn(channel, key, DEFAULT_VELOCITY)

    if (useTransportStore.getState().playing || state.cursor === null) return
    const context = currentSession()?.editor.context
    if (!context || context.channel !== channel) return
    const start = state.cursor
    const meter = patternMeterAt(
      start,
      context.pattern.signature,
      context.pattern.timeline
    )
    const length =
      snapTicks(liveInputSnap(), meter.signature) || TICKS_PER_STEP
    if (start + length > MAX_PATTERN_TICKS) return
    // Reserve the next position synchronously, even when keys arrive before IPC replies.
    useStepEntryStore.setState({ cursor: start + length })
    void dispatch({
      type: "addNotes",
      pattern: context.pattern.id,
      channel,
      notes: [{ start, length, key, velocity: DEFAULT_VELOCITY }],
    })
  }
  const onKeyUp = (event: KeyboardEvent) => {
    const id = identity(event)
    const key = held.get(id)
    if (key === undefined) return
    held.delete(id)
    auditionOff(channel, key)
    event.preventDefault()
    event.stopImmediatePropagation()
  }
  const stop = useStepEntryStore.subscribe((state) => {
    if (!state.enabled) release()
  })
  root.addEventListener("keydown", onKeyDown, true)
  root.addEventListener("focusout", release)
  window.addEventListener("keyup", onKeyUp, true)
  window.addEventListener("blur", release)
  return () => {
    release()
    stop()
    root.removeEventListener("keydown", onKeyDown, true)
    root.removeEventListener("focusout", release)
    window.removeEventListener("keyup", onKeyUp, true)
    window.removeEventListener("blur", release)
  }
}
