import type { PlaylistTrackId, Project, TrackKind } from "@/bindings"
import type { ContextItem } from "@/components/context-actions"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"

import { usePlaylistStore } from "./store"

/*
 * Linked tracks: a playlist track that behaves like a track of its own
 * kind. An instrument track plays only its channel of the patterns placed
 * on it; an audio track sends every audio clip placed on it to its mixer
 * track. The document keeps both rules, so the link is all the UI sets.
 */

/** The link a track has, or null. */
export function trackLink(
  project: Project,
  track: PlaylistTrackId
): TrackKind | null {
  return project.playlist.arrangementBook?.linkedTracks[track] ?? null
}

/** What a header says about a track's link, or null without one. */
export function trackLinkLabel(
  project: Project,
  track: PlaylistTrackId
): string | null {
  const link = trackLink(project, track)
  if (!link) return null
  const name =
    link.type === "instrument"
      ? project.channels.find((channel) => channel.id === link.channel)?.name
      : link.type === "audioTrack"
        ? project.mixer.tracks.find((item) => item.id === link.mixer)?.name
        : project.samples.find((sample) => sample.id === link.source)?.name
  return name ?? "Missing link"
}

/** Links a track, or with null unlinks it. One undo step. */
export function linkTrack(track: PlaylistTrackId, kind: TrackKind | null) {
  return dispatch({ type: "linkTrack", track, kind })
}

/**
 * Selects what a linked track stands for, as pressing its header does: the
 * channel and its mixer track for an instrument track, the mixer track for
 * an audio track. Nothing is dispatched.
 */
export function selectLinkedSource(track: PlaylistTrackId) {
  const project = useProjectStore.getState().project
  const link = trackLink(project, track)
  if (!link) return
  const ui = useUiStore.getState()
  if (link.type === "instrument") {
    const channel = project.channels.find((item) => item.id === link.channel)
    if (!channel) return
    ui.selectChannel(channel.id)
    ui.selectTrack(channel.mixerTrack)
  } else if (link.type === "audioTrack") {
    if (project.mixer.tracks.some((item) => item.id === link.mixer))
      ui.selectTrack(link.mixer)
  }
}

/** A channel with its mixer track and a track linked to it, in one step. */
export async function addInstrumentTrack(): Promise<void> {
  const added = await dispatch({ type: "addInstrumentTrack" })
  // The playlist track is the last id the command creates.
  const track = added?.created.at(-1)
  if (track === undefined) return
  usePlaylistStore.getState().setTargetTrack(track)
  selectLinkedSource(track)
}

/** A mixer track and a track linked to it, in one step. */
export async function addAudioTrack(): Promise<void> {
  const added = await dispatch({ type: "addAudioTrack" })
  const track = added?.created.at(-1)
  if (track === undefined) return
  usePlaylistStore.getState().setTargetTrack(track)
  selectLinkedSource(track)
}

/** The "Link track" submenu of a track header. */
export function linkMenu(track: PlaylistTrackId): ContextItem {
  const project = useProjectStore.getState().project
  const link = trackLink(project, track)
  return {
    submenu: "Link track",
    items: [
      {
        title: "Not linked",
        checked: link === null,
        disabled: link === null,
        run: () => linkTrack(track, null),
      },
      { label: "Instrument track for" },
      ...project.channels.map((channel) => ({
        title: channel.name,
        checked: link?.type === "instrument" && link.channel === channel.id,
        run: () => linkTrack(track, { type: "instrument", channel: channel.id }),
      })),
      { label: "Audio track into" },
      ...project.mixer.tracks
        .filter((item) => !item.current)
        .map((item) => ({
          title: item.name,
          checked: link?.type === "audioTrack" && link.mixer === item.id,
          run: () => linkTrack(track, { type: "audioTrack", mixer: item.id }),
        })),
    ],
  }
}
