import { create } from "zustand"
import { backend } from "@/lib/ipc"
import { getProjectGeneration } from "@/lib/store/replaced"
import { addAudioFile } from "@/features/playlist/audio/ops"

export const useRemixLoader = create(() => ({ busy: false }))

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
    return await addAudioFile(path, { start: 0 }, undefined, valid)
  } finally {
    useRemixLoader.setState({ busy: false })
  }
}
