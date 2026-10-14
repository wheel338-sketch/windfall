import type { Project } from "@/bindings"
import { targetState } from "@/lib/automation/targets"
import { useHintStore } from "@/lib/store/hint"
import { useProjectStore } from "@/lib/store/project"
import { automatedValue, realtimeFrame } from "@/lib/store/realtime"

/*
 * Editing a control that automation is moving right now changes the value
 * the project stores for it, which is what it has outside its automation
 * clips. Under a clip that edit is not heard, and the knob does not follow
 * the hand. Nothing stops it, but the status bar says what is going on.
 */

/** How long the notice stays after the last such edit. */
export const NOTICE_MS = 5000

export function automatedEditNotice(name: string): string {
  return `Automated by ${name} — this sets the value used outside its clips`
}

/**
 * The name of an automation whose target got a new stored value between
 * two states of the project while the automation had it in hand. Null when
 * no automated control was edited.
 */
export function editedWhileAutomated(
  before: Project,
  after: Project
): string | null {
  if (before === after || realtimeFrame().automated.length === 0) return null
  for (const automation of after.automations) {
    if (automatedValue(automation.id) === undefined) continue
    const was = targetState(before, automation.target)
    const now = targetState(after, automation.target)
    if (was && now && was.stored !== now.stored) return automation.name
  }
  return null
}

/**
 * Watches the project for edits to controls that automation is moving,
 * and shows the notice in the status bar for a while after each. Returns a
 * function that stops.
 */
export function watchAutomatedEdits(): () => void {
  let timer: ReturnType<typeof setTimeout> | null = null
  let shown: string | null = null
  const clear = () => {
    if (timer !== null) clearTimeout(timer)
    timer = null
    if (shown !== null && useHintStore.getState().notice === shown) {
      useHintStore.setState({ notice: null })
    }
    shown = null
  }
  const stop = useProjectStore.subscribe((state, previous) => {
    const name = editedWhileAutomated(previous.project, state.project)
    if (name === null) return
    if (timer !== null) clearTimeout(timer)
    shown = automatedEditNotice(name)
    useHintStore.setState({ notice: shown })
    timer = setTimeout(clear, NOTICE_MS)
  })
  return () => {
    stop()
    clear()
  }
}
