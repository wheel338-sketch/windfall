import type { AutomationTarget, ClipId, PatternId } from "@/bindings"
import { openEffect } from "@/features/mixer/effect-ops"
import { selectChannel } from "@/features/channel-rack/channel-ops"
import { usePlaylistStore } from "@/features/playlist/store"
import { useUiStore } from "@/lib/store/ui"
import { setTransportPattern } from "@/lib/store/transport"
import { useProjectStore } from "@/lib/store/project"

export async function navigatePattern(pattern: PatternId, channel?: number) {
  if (!useProjectStore.getState().project.patterns.some((p) => p.id === pattern)) return
  await setTransportPattern(pattern)
  if (channel !== undefined) selectChannel(channel)
  useUiStore.getState().showCenterTab(channel === undefined ? "channelRack" : "pianoRoll")
}
export function navigateClip(id: ClipId) {
  const clip = useProjectStore.getState().project.playlist.clips.find((clip) => clip.id === id)
  if (!clip) return
  const ui = usePlaylistStore.getState()
  ui.select([id]); ui.setTargetTrack(clip.track); ui.setCursorTick(clip.start); ui.setReveal(id); ui.requestFocus()
  useUiStore.getState().showCenterTab("playlist")
}
export function navigateTarget(target: AutomationTarget) {
  if ("channel" in target) {
    selectChannel(target.channel, { openSettings: true })
    useUiStore.getState().showCenterTab("channelRack")
  } else if ("effect" in target) openEffect(target.effect)
  else if ("track" in target) {
    useUiStore.getState().selectTrack(target.track)
    useUiStore.getState().setPanelVisible("mixer", true)
  } else useUiStore.getState().showCenterTab("playlist")
}
