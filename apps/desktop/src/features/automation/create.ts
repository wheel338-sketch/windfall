import type { AutomationId, AutomationTarget } from "@/bindings"
import { revealClip } from "@/features/playlist/reveal"
import { usePlaylistStore } from "@/features/playlist/store"
import { attempt } from "@/lib/errors"
import { backend } from "@/lib/ipc"
import { receivePatch, useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { translatedText as t } from "@/lib/i18n"

/**
 * Makes an automation of a target and puts it on the playlist: what
 * "Create automation clip" on a knob or fader does. One undo step. The
 * playlist comes forward with the new clip selected and in view, ready for
 * its curve to be drawn, and takes the keyboard: the keys that follow are
 * for the clip, not for the control the menu was opened on.
 */
export async function createAutomationClip(
  target: AutomationTarget
): Promise<AutomationId | null> {
  const result = await attempt(
    backend.automate(target),
    t("Could not create the automation clip")
  )
  if (!result) return null
  // The same patch also arrives as an event; the store ignores the repeat.
  receivePatch(result.patch)
  const [automation, , clip] = result.created
  if (clip !== undefined) revealClip(clip, { focus: true })
  return automation ?? null
}

/**
 * Shows an automation on the playlist: its first clip, selected and in
 * view, with the keyboard on the timeline. One that has no clip is made
 * the brush instead, so the next click on the timeline places one.
 */
export function showAutomation(id: AutomationId): void {
  const clip = useProjectStore
    .getState()
    .project.playlist.clips.find(
      (item) =>
        item.content.type === "automation" && item.content.automation === id
    )
  if (clip) {
    revealClip(clip.id, { focus: true })
    return
  }
  const playlist = usePlaylistStore.getState()
  playlist.setBrush({ type: "automation", automation: id })
  playlist.requestFocus()
  useUiStore.getState().showCenterTab("playlist")
}
