import { historyLabel } from "@/lib/i18n"
import { dispatch } from "@/lib/store/project"

import { buildTrackRows } from "./track-rows"
import { playlist } from "./selectors"

/** All descendant tracks change together, including members hidden by collapse. */
export async function toggleGroupMute(group: number): Promise<void> {
  const header = buildTrackRows(playlist(), new Set()).rows.find(
    (row) => row.kind === "group" && row.group.id === group
  )
  if (header?.kind !== "group" || header.members.length === 0) return
  const muted = header.members.some((track) => !track.muted)
  await dispatch({
    type: "batch",
    label: muted ? historyLabel("Mute playlist track group") : historyLabel("Unmute playlist track group"),
    commands: header.members.map((track) => ({
      type: "updatePlaylistTrack",
      id: track.id,
      patch: { muted },
    })),
  })
}

/** All descendant tracks change together, including members hidden by collapse. */
export async function toggleGroupSolo(group: number): Promise<void> {
  const header = buildTrackRows(playlist(), new Set()).rows.find(
    (row) => row.kind === "group" && row.group.id === group
  )
  if (header?.kind !== "group" || header.members.length === 0) return
  const solo = header.members.some((track) => !track.solo)
  await dispatch({
    type: "batch",
    label: solo ? historyLabel("Solo playlist track group") : historyLabel("Unsolo playlist track group"),
    commands: header.members.map((track) => ({
      type: "updatePlaylistTrack",
      id: track.id,
      patch: { solo },
    })),
  })
}
