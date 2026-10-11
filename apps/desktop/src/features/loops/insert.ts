import { backend } from "@/lib/ipc"
import { reportError } from "@/lib/errors"
import { receivePatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { revealClip } from "@/features/playlist/reveal"
import type { LoopDrag } from "./drag"

export async function insertFactoryLoop(loop: LoopDrag, start: number, track: number | null = null): Promise<boolean> {
  const localGeneration = getProjectGeneration()
  try {
    const result = await backend.loopInsert(loop.id, loop.key, track, Math.max(0, Math.round(start)), loop.generation, loop.revision)
    if (localGeneration !== getProjectGeneration()) return false
    receivePatch(result.patch)
    // IDs are global, and clips are song-ordered; never assume the last clip.
    const created = result.created.find((id) => useProjectStore.getState().project.playlist.clips.some((clip) => clip.id === id))
    if (created !== undefined) revealClip(created)
    return true
  } catch (error) { reportError(error, "Could not insert the factory loop"); return false }
}
