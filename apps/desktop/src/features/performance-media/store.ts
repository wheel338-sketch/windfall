import { create } from "zustand"
import type { StageMedia } from "@/bindings"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { backend } from "@/lib/ipc"
import { translatedText as t } from "@/lib/i18n"

export const DEFAULT_STAGE_MEDIA: StageMedia = {
  pads: [], takes: [], quantization: "bar", videos: [], visual: { scene: "orbit", intensity: 1, hue: 180 }, danceChannel: null,
}
export const useMediaWindows = create<{ performance: boolean; visualizer: boolean; video: boolean }>(() => ({ performance: false, visualizer: false, video: false }))
onProjectReplaced(() => useMediaWindows.setState({ performance: false, visualizer: false, video: false }))

export function changeStageMedia(change: (media: StageMedia) => StageMedia) {
  const current = useProjectStore.getState().project.settings.stageMedia ?? DEFAULT_STAGE_MEDIA
  return dispatch({ type: "setStageMedia", media: change(current) })
}

/** The window's cancellation counter is not the native document identity:
 * reloads and detached windows may have seen different replacement counts. */
export async function mediaDocumentGuard(generation = getProjectGeneration(), revision = useProjectStore.getState().revision) {
  const guard = await backend.timelineState()
  if (generation !== getProjectGeneration() || revision !== useProjectStore.getState().revision || guard.revision !== revision) throw new Error(t("The project changed before this media operation"))
  return { generation: guard.generation, revision }
}
