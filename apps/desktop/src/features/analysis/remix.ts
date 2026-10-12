import { create } from "zustand"
import { backend } from "@/lib/ipc"
import { getProjectGeneration } from "@/lib/store/replaced"
import { addAudioFile } from "@/features/playlist/audio/ops"
import { useProjectStore } from "@/lib/store/project"

export const useRemixLoader = create(() => ({ busy: false, clip: null as number | null,
  tempoCandidates: [] as { bpm: number; confidence: number }[], tempoProblem: null as string | null }))

/** Reuse ordinary audio import, its native stale checks and one-step history.
 * The later reviewed stem import is a separate atomic undo step. */
export async function loadRemixSong(current: () => boolean): Promise<number | null> {
  if (useRemixLoader.getState().busy || !current()) return null
  const generation = getProjectGeneration()
  const valid = () => generation === getProjectGeneration() && current()
  useRemixLoader.setState({ busy: true })
  try {
    const path = await backend.pickAudioFile()
    if (!path || !valid()) return null
    const id = await addAudioFile(path, { start: 0 }, undefined, valid)
    if (id === null || !valid()) return null
    const clip = useProjectStore.getState().project.playlist.clips.find((c) => c.id === id)
    if (!clip || clip.content.type !== "audio") return null
    useRemixLoader.setState({ clip: id, tempoCandidates: [], tempoProblem: null })
    try {
      const tempoCandidates = await backend.detectClipTempo(clip.content.sample)
      if (valid()) useRemixLoader.setState({ tempoCandidates })
    } catch (e) {
      if (valid()) useRemixLoader.setState({ tempoProblem: e instanceof Error ? e.message : String(e) })
    }
    return valid() ? id : null
  } finally {
    useRemixLoader.setState({ busy: false })
  }
}
