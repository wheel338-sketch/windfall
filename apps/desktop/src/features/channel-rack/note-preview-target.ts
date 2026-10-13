import type { ChannelId, PatternId } from "@/bindings"
import {
  getAppState,
  isEnabled,
  runAction,
  type Action,
  type AppState,
} from "@/lib/actions"
import { getProjectGeneration } from "@/lib/store/replaced"
import { selectedPatternId } from "@/lib/store/selectors"
import { useUiStore } from "@/lib/store/ui"
import { translatedText as t } from "@/lib/i18n"

export type NotePreviewLane = { pattern: PatternId; channel: ChannelId }
export type NotePreviewTarget = NotePreviewLane & { generation: number }

/** Palette and keymap commands act on the current selected lane. */
export function selectedNotePreviewLane(
  state: AppState
): NotePreviewLane | null {
  const { project } = state.document
  const channel = state.ui.selectedChannel
  const pattern = selectedPatternId(project, state.transport.pattern)
  return channel !== null &&
    pattern !== null &&
    project.channels.some((item) => item.id === channel) &&
    project.patterns.some((item) => item.id === pattern)
    ? { pattern, channel }
    : null
}

/** A captured menu/press is never allowed to act on a successor document/lane. */
export function notePreviewTargetReason(
  target: NotePreviewTarget,
  state = getAppState()
): string | undefined {
  if (target.generation !== getProjectGeneration()) return t("Project changed")
  const { project } = state.document
  if (!project.channels.some((item) => item.id === target.channel))
    return t("Channel no longer exists")
  if (!project.patterns.some((item) => item.id === target.pattern))
    return t("Pattern no longer exists")
  if (selectedPatternId(project, state.transport.pattern) !== target.pattern)
    return t("Pattern changed")
}

function targetState(state: AppState, target: NotePreviewTarget): AppState {
  return { ...state, ui: { ...state.ui, selectedChannel: target.channel } }
}

/** Metadata stays canonical; only the selection/lifetime context is bound. */
export function notePreviewActionForTarget(
  action: Action,
  target: NotePreviewTarget
): Action {
  return {
    ...action,
    enabled: (state) =>
      !notePreviewTargetReason(target, state) &&
      isEnabled(action, targetState(state, target)),
    whyDisabled: (state) =>
      notePreviewTargetReason(target, state) ??
      action.whyDisabled?.(targetState(state, target)),
    checked: action.checked
      ? (state) => action.checked!(targetState(state, target))
      : undefined,
    run: async () => {
      await runNotePreviewAction(target, action)
    },
  }
}

/** Called in capture before a shared ActionMenuItem executes its registry id. */
export function prepareNotePreviewTarget(target: NotePreviewTarget): boolean {
  if (notePreviewTargetReason(target)) return false
  useUiStore.getState().selectChannel(target.channel)
  return !notePreviewTargetReason(target)
}

/** A view switch can replace the focused control. Focus after React commits,
 * and recheck the captured document/lane so a pending frame cannot select a
 * successor project's reused id. This is one frame per command, not playback.
 */
export function focusNotePreviewTarget(target: NotePreviewTarget): void {
  requestAnimationFrame(() => {
    if (
      notePreviewTargetReason(target) ||
      getAppState().ui.selectedChannel !== target.channel
    )
      return
    document
      .querySelector<HTMLElement>(
        `[data-channel-row="${target.channel}"] [data-slot="step-grid"] button[tabindex="0"]`
      )
      ?.focus({ preventScroll: true })
  })
}

export async function runNotePreviewAction(
  target: NotePreviewTarget,
  action: Action
): Promise<boolean> {
  if (!isEnabled(notePreviewActionForTarget(action, target), getAppState()))
    return false
  if (!prepareNotePreviewTarget(target)) return false
  await runAction(action.id)
  return true
}
