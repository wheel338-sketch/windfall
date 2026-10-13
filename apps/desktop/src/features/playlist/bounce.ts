import { attempt } from "@/lib/errors"
import { backend } from "@/lib/ipc"
import { receivePatch } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"

import { selectedClips } from "./selectors"
import { usePlaylistStore } from "./store"
import { translatedText as t } from "@/lib/i18n"

/** The backend renders and commits one batch; the desktop mirrors its patch. */
export async function bounceSelectedClips(): Promise<void> {
  const clips = selectedClips()
  if (clips.length === 0) return
  const generation = getProjectGeneration()
  const result = await attempt(
    backend.bounceSelectedClips(clips.map((clip) => clip.id)),
    t("Could not bounce the selected clips")
  )
  if (!result || generation !== getProjectGeneration()) return
  receivePatch(result.patch)
  const clip = result.created.at(-1)
  if (clip !== undefined) usePlaylistStore.getState().select([clip])
}
