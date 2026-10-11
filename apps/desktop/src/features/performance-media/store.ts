import { create } from "zustand"
import type { StageMedia } from "@/bindings"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { onProjectReplaced } from "@/lib/store/replaced"

export const DEFAULT_STAGE_MEDIA: StageMedia = {
  pads: [], quantization: "bar", videos: [], visual: { scene: "orbit", intensity: 1, hue: 180 }, danceChannel: null,
}
export const useMediaWindows = create<{ performance: boolean; visualizer: boolean; video: boolean }>(() => ({ performance: false, visualizer: false, video: false }))
onProjectReplaced(() => useMediaWindows.setState({ performance: false, visualizer: false, video: false }))

export function changeStageMedia(change: (media: StageMedia) => StageMedia) {
  const current = useProjectStore.getState().project.settings.stageMedia ?? DEFAULT_STAGE_MEDIA
  return dispatch({ type: "setStageMedia", media: change(current) })
}
