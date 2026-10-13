import { create } from "zustand"

import type { AutomationTarget, PatternId } from "@/bindings"
import { registry, type Action } from "@/lib/actions"
import { useProjectStore } from "@/lib/store/project"
import { onProjectReplaced } from "@/lib/store/replaced"
import { useTransportStore } from "@/lib/store/transport"
import { dynamicTitle } from "@/lib/i18n"

/*
 * The event editor: a window of its own for the automation a pattern
 * stores as events. Which pattern and control it shows is view state: it is
 * not saved and not part of undo.
 */

type EventEditorState = {
  open: boolean
  /** Null follows the pattern the transport has selected. */
  pattern: PatternId | null
  target: AutomationTarget | null
}

export const useEventEditor = create<EventEditorState>(() => ({
  open: false,
  pattern: null,
  target: null,
}))

export function closeEventEditor(): void {
  useEventEditor.setState({ open: false })
}

/** A replaced project can reuse ids, so nothing of the old view is kept. */
onProjectReplaced(() =>
  useEventEditor.setState({ open: false, pattern: null, target: null })
)

/** The pattern the editor shows: its own choice, else the selected pattern. */
export function shownPattern(chosen: PatternId | null): PatternId | null {
  const { project } = useProjectStore.getState()
  if (chosen !== null && project.patterns.some((item) => item.id === chosen))
    return chosen
  const selected = useTransportStore.getState().pattern
  if (project.patterns.some((item) => item.id === selected)) return selected
  return project.patterns[0]?.id ?? null
}

/**
 * Opens the editor. With a target it shows that control's lane in the
 * selected pattern, which is what "Edit events in pattern" on a knob or
 * fader does; without one it keeps what it showed last.
 */
export function openEventEditor(
  target?: AutomationTarget,
  pattern?: PatternId
): void {
  useEventEditor.setState((state) => ({
    open: true,
    pattern: pattern ?? state.pattern,
    target: target && target.type !== "tempo" ? target : state.target,
  }))
}

export function showEventPattern(pattern: PatternId): void {
  useEventEditor.setState({ pattern })
}

export function showEventTarget(target: AutomationTarget | null): void {
  useEventEditor.setState({ target })
}

const EVENT_EDITOR_ACTIONS: Action[] = [
  {
    id: "view.event-editor",
    ...dynamicTitle("Show event editor", {}),
    section: "View",
    keywords:
      "event automation pattern events lane curve knob fader parameter edit",
    run: () => openEventEditor(),
  },
]

export function registerEventEditorActions(): () => void {
  const unregister = registry.register(EVENT_EDITOR_ACTIONS)
  return () => {
    unregister()
    closeEventEditor()
  }
}
