import type { Clip, PlaylistTrackId } from "@/bindings"
import { attempt } from "@/lib/errors"
import { backend } from "@/lib/ipc"
import { receivePatch } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"

import { arrangementPlaylist } from "./arrangement-view"
import { playlist, selectedClips } from "./selectors"
import { usePlaylistStore } from "./store"
import { translatedText as t } from "@/lib/i18n"

/** A clip a consolidation prints: sound, not muted, in the layout shown. */
const printable = (clip: Clip) =>
  !clip.muted && clip.length > 0 && clip.content.type !== "automation"

/** The tracks that hold something to print, in playlist order. */
export function consolidatableTracks(
  tracks: readonly PlaylistTrackId[]
): PlaylistTrackId[] {
  const wanted = new Set(tracks)
  const shown = arrangementPlaylist(playlist())
  const holding = new Set(
    shown.clips.filter(printable).map((clip) => clip.track)
  )
  return shown.tracks
    .filter((track) => wanted.has(track.id) && holding.has(track.id))
    .map((track) => track.id)
}

/** The tracks the selected clips sit on. */
export function selectedClipTracks(): PlaylistTrackId[] {
  return [...new Set(selectedClips().map((clip) => clip.track))]
}

/**
 * Renders each track's unmuted pattern and audio clips to one audio clip on
 * that track and mutes the sources. The backend commits one batch for all
 * of them; the prints are selected when it is done.
 */
export async function consolidateTracks(
  tracks: readonly PlaylistTrackId[]
): Promise<void> {
  const chosen = consolidatableTracks(tracks)
  if (chosen.length === 0) return
  const generation = getProjectGeneration()
  const result = await attempt(
    backend.consolidatePlaylistTracks(chosen),
    chosen.length === 1
      ? t("Could not consolidate the track")
      : t("Could not consolidate the tracks")
  )
  if (!result || generation !== getProjectGeneration()) return
  receivePatch(result.patch)
  // `created` lists a sample and then its clip for every track.
  const made = new Set(result.created)
  const prints = playlist()
    .clips.filter((clip) => made.has(clip.id))
    .map((clip) => clip.id)
  if (prints.length > 0) usePlaylistStore.getState().select(prints)
}
