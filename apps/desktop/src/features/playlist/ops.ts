import type {
  ClipContent,
  ClipId,
  Command,
  PatternId,
  PlaylistTrackId,
} from "@/bindings"
import { runAction } from "@/lib/actions"
import { refuse } from "@/lib/errors"
import { newGestureId } from "@/lib/store/gesture"
import { dispatch } from "@/lib/store/project"
import { askConfirm, askText } from "@/lib/store/prompts"
import { realtimeFrame } from "@/lib/store/realtime"
import {
  play,
  seek,
  setPlayMode,
  setTransport,
  setTransportPattern,
  useTransportStore,
} from "@/lib/store/transport"

import {
  clampNudge,
  clipboardFrom,
  clipInits,
  clipUpdates,
  duplicateRight,
  moveChanges,
  pasteAt,
  spanFits,
  tracksNeeded,
  type ClipChange,
  type NewClip,
} from "./edit"
import { rowCountFor } from "./layout"
import { playlist, project, selectedClips } from "./selectors"
import { nudgeTicks, snapTicks } from "./snap"
import { usePlaylistStore } from "./store"
import { documentRowFor, trackRowsNow } from "./track-rows"
import { historyLabel, translatedText as t, message, plainNumber } from "@/lib/i18n"

/*
 * Everything the playlist does to the project and the transport. Each
 * function is one undo step, whatever it takes to get there.
 */

const ui = () => usePlaylistStore.getState()

/** One command under a history label of our own choosing. */
function labelled(label: string, ...commands: Command[]): Command {
  return { type: "batch", label, commands }
}

export function currentSnapTicks(): number {
  return snapTicks(ui().snap, project().settings.timeSignature)
}

export function rowOfTrack(): (track: PlaylistTrackId) => number {
  const { trackRows: rows } = trackRowsNow()
  return (track) => rows.get(track) ?? 0
}

/**
 * Makes sure every row has a track, adding "Track N" tracks at the bottom
 * for the rows that have none. A command cannot use an id made earlier in
 * the same batch, so the tracks are added first and whatever uses them is
 * sent after with the same gesture id, which keeps it all one undo step.
 */
async function ensureTracks(
  rows: Iterable<number>,
  gesture: number,
  label: string
): Promise<PlaylistTrackId[] | null> {
  const missing = tracksNeeded(playlist().tracks.length, rows)
  if (missing > 0) {
    const added = await dispatch(
      labelled(
        label,
        ...Array.from({ length: missing }, (): Command => ({
          type: "addPlaylistTrack",
        }))
      ),
      gesture
    )
    if (!added) return null
  }
  return playlist().tracks.map((track) => track.id)
}

/**
 * Turns down an edit that would put a clip past the last tick the timeline
 * can count. Without this the clip would be pulled back to that tick, and
 * a few rounds of duplicating would pile clips up there.
 */
function refusePastEnd() {
  refuse(
    t("Clips cannot go past the end of the timeline"),
    t("The song is as long as it can get.")
  )
}

/** Adds clips, with the tracks they need. Resolves to the new clip ids. */
export async function addClips(
  clips: readonly NewClip[],
  label: string
): Promise<ClipId[] | null> {
  if (clips.length === 0) return []
  const layout = trackRowsNow()
  const destinations = clips.map((clip) => documentRowFor(clip.row, layout))
  if (destinations.some((row) => row === undefined)) return null
  clips = clips.map((clip, index) => ({ ...clip, row: destinations[index]! }))
  if (!clips.every((clip) => spanFits(clip.start, clip.length))) {
    refusePastEnd()
    return null
  }
  const gesture = newGestureId()
  const tracks = await ensureTracks(
    clips.map((clip) => clip.row),
    gesture,
    label
  )
  if (!tracks) return null
  const added = await dispatch(
    labelled(label, { type: "addClips", clips: clipInits(clips, tracks) }),
    gesture
  )
  return added?.created ?? null
}

/** Changes clips, adding tracks for any that move below the last one. */
export async function changeClips(
  changes: readonly ClipChange[],
  label: string
): Promise<boolean> {
  if (changes.length === 0) return true
  const layout = trackRowsNow()
  const destinations = changes.map((change) =>
    change.row === undefined ? undefined : documentRowFor(change.row, layout)
  )
  if (
    changes.some(
      (change, index) =>
        change.row !== undefined && destinations[index] === undefined
    )
  )
    return false
  changes = changes.map((change, index) => ({
    ...change,
    row: destinations[index],
  }))
  const byId = new Map(playlist().clips.map((clip) => [clip.id, clip]))
  const fits = changes.every((change) => {
    const clip = byId.get(change.id)
    if (!clip) return true
    const start = change.start ?? clip.start
    const length = change.length ?? clip.length
    // A clip may always come back towards the start.
    return spanFits(start, length) || start + length <= clip.start + clip.length
  })
  if (!fits) {
    refusePastEnd()
    return false
  }
  const gesture = newGestureId()
  const tracks = await ensureTracks(
    changes.flatMap((change) => change.row ?? []),
    gesture,
    label
  )
  if (!tracks) return false
  const done = await dispatch(
    labelled(label, {
      type: "updateClips",
      updates: clipUpdates(changes, tracks),
    }),
    gesture
  )
  return done !== null
}

export async function deleteClips(ids: readonly ClipId[]): Promise<void> {
  const existing = new Set(playlist().clips.map((clip) => clip.id))
  const clips = ids.filter((id) => existing.has(id))
  if (clips.length === 0) return
  const removed = new Set(clips)
  const book = playlist().arrangementBook
  const cleanup: Command[] = []
  for (const group of book?.clipGroups ?? []) {
    if (group.clips.every((id) => removed.has(id))) {
      cleanup.push({ type: "removeClipGroup", id: group.id })
    }
  }
  if (cleanup.length > 0) {
    // Group erasure must also leave saved arrangement references valid.
    for (const arrangement of book?.arrangements ?? []) {
      if (arrangement.clips.some((id) => removed.has(id))) {
        cleanup.push({
          type: "setArrangementReferences",
          id: arrangement.id,
          clips: arrangement.clips.filter((id) => !removed.has(id)),
          tracks: arrangement.tracks,
        })
      }
    }
  }
  await dispatch(
    labelled(historyLabel("Delete clips", { count: clips.length }), ...cleanup, {
      type: "removeClips",
      clips,
    })
  )
}

export async function setClipsMuted(
  ids: readonly ClipId[],
  muted: boolean
): Promise<void> {
  const wanted = new Set(ids)
  const changes = playlist()
    .clips.filter((clip) => wanted.has(clip.id) && clip.muted !== muted)
    .map((clip) => ({ id: clip.id, muted }))
  const count = changes.length
  await changeClips(
    changes,
    muted ? historyLabel("Mute clips", { count }) : historyLabel("Unmute clips", { count })
  )
}

export function selectAll(): void {
  ui().select(playlist().clips.map((clip) => clip.id))
}

export async function deleteSelection(): Promise<void> {
  await deleteClips(selectedClips().map((clip) => clip.id))
  ui().select(selectedClips().map((clip) => clip.id))
}

/** Mutes the selected clips, or unmutes them when all are muted already. */
export async function toggleMuteSelection(): Promise<void> {
  const clips = selectedClips()
  if (clips.length === 0) return
  await setClipsMuted(
    clips.map((clip) => clip.id),
    !clips.every((clip) => clip.muted)
  )
}

export function copySelection(): boolean {
  const clips = selectedClips()
  if (clips.length === 0) return false
  ui().setClipboard(clipboardFrom(clips, rowOfTrack()))
  return true
}

export async function cutSelection(): Promise<void> {
  const clips = selectedClips()
  if (!copySelection()) return
  await dispatch(
    labelled(historyLabel("Cut clips", { count: clips.length }), {
      type: "removeClips",
      clips: clips.map((clip) => clip.id),
    })
  )
  ui().select(selectedClips().map((clip) => clip.id))
}

/**
 * Where the song is: the playhead in song mode. In pattern mode the engine
 * keeps no song position, so it is the place last set on the ruler.
 */
export function songTick(): number {
  return useTransportStore.getState().mode === "song"
    ? realtimeFrame().tick
    : ui().cursorTick
}

/** Whether what a clip plays is still in the project. */
export function contentExists(content: ClipContent): boolean {
  const current = project()
  switch (content.type) {
    case "pattern":
      return current.patterns.some((item) => item.id === content.pattern)
    case "audio":
      return current.samples.some((item) => item.id === content.sample)
    case "automation":
      return current.automations.some((item) => item.id === content.automation)
    default: {
      const _exhaustive: never = content
      return _exhaustive
    }
  }
}

/**
 * A copy of an audio clip plays into the track the original did. If that
 * track is gone by the time of the paste, the copy plays into the master,
 * as the clips that were on the track do.
 */
function withLiveMixerTrack(clip: NewClip): NewClip {
  const content = clip.content
  if (content.type !== "audio") return clip
  const live = project().mixer.tracks.some(
    (track) => track.id === content.mixerTrack
  )
  return live ? clip : { ...clip, content: { ...content, mixerTrack: 0 } }
}

/** Pastes at the song position and selects what was pasted. */
export async function paste(): Promise<void> {
  const clips = pasteAt(
    ui().clipboard,
    songTick(),
    currentSnapTicks(),
    contentExists
  ).map(withLiveMixerTrack)
  const created = await addClips(clips, historyLabel("Paste clips", { count: clips.length }))
  if (created && created.length > 0) ui().select(created)
}

/** Copies the selection to right after itself and selects the copies. */
export async function duplicateSelection(): Promise<void> {
  const clips = duplicateRight(
    selectedClips(),
    rowOfTrack(),
    currentSnapTicks()
  )
  const created = await addClips(clips, historyLabel("Duplicate clips", { count: clips.length }))
  if (created && created.length > 0) ui().select(created)
}

/** Moves the selection by whole grid cells and rows, as the arrow keys do. */
export async function nudgeSelection(
  cells: number,
  rows: number
): Promise<void> {
  const clips = selectedClips()
  if (clips.length === 0) return
  const rowOf = rowOfTrack()
  const step = nudgeTicks(ui().snap, project().settings.timeSignature)
  const move = clampNudge(
    clips.map((clip) => ({ start: clip.start, row: rowOf(clip.track) })),
    cells * step,
    rows,
    rowCountFor(trackRowsNow().rows.length, 0)
  )
  await changeClips(
    moveChanges(clips, rowOf, move.ticks, move.rows),
    historyLabel("Move clips", { count: clips.length })
  )
}

/** Selects a clip's pattern and shows it in the channel rack for editing. */
export async function openPattern(pattern: PatternId): Promise<void> {
  await setTransportPattern(pattern)
  await runAction("view.channelRack")
}

export async function addTrack(): Promise<void> {
  const added = await dispatch({ type: "addPlaylistTrack" })
  if (added) ui().setTargetTrack(added.created[0])
}

/** Puts a new empty track at `index`, pushing the tracks from there down. */
export async function insertTrack(index: number): Promise<void> {
  if (index >= playlist().tracks.length) return addTrack()
  const added = await dispatch(
    labelled("Insert track", { type: "addPlaylistTrack", index })
  )
  if (added) ui().setTargetTrack(added.created[0])
}

/** Moves a track, with its clips, to another place among the tracks. */
export async function moveTrack(
  id: PlaylistTrackId,
  index: number
): Promise<void> {
  const tracks = playlist().tracks
  const from = tracks.findIndex((track) => track.id === id)
  const to = Math.min(Math.max(0, index), tracks.length - 1)
  if (from < 0 || to === from) return
  await dispatch({ type: "movePlaylistTrack", id, index: to })
}

/** Moves a track one place up (-1) or down (1). */
export function moveTrackBy(id: PlaylistTrackId, step: -1 | 1): Promise<void> {
  const from = playlist().tracks.findIndex((track) => track.id === id)
  return from < 0 ? Promise.resolve() : moveTrack(id, from + step)
}

export async function setTrackName(
  id: PlaylistTrackId,
  name: string
): Promise<void> {
  const track = playlist().tracks.find((item) => item.id === id)
  const trimmed = name.trim()
  if (!track || trimmed === "" || trimmed === track.name) return
  await dispatch({
    type: "updatePlaylistTrack",
    id,
    patch: { name: trimmed },
  })
}

export async function renameTrack(id: PlaylistTrackId): Promise<void> {
  const track = playlist().tracks.find((item) => item.id === id)
  if (!track) return
  const name = await askText({
    title: t("Rename track"),
    label: t("Name"),
    initial: track.name,
    submitLabel: t("Rename"),
  })
  if (name !== null) await setTrackName(id, name)
}

export async function toggleTrackMute(id: PlaylistTrackId): Promise<void> {
  const track = playlist().tracks.find((item) => item.id === id)
  if (!track) return
  await dispatch({
    type: "updatePlaylistTrack",
    id,
    patch: { muted: !track.muted },
  })
}

export async function toggleTrackSolo(id: PlaylistTrackId): Promise<void> {
  const track = playlist().tracks.find((item) => item.id === id)
  if (!track) return
  await dispatch({
    type: "updatePlaylistTrack",
    id,
    patch: { solo: !track.solo },
  })
}

export async function setTrackColor(
  id: PlaylistTrackId,
  color: number
): Promise<void> {
  const track = playlist().tracks.find((item) => item.id === id)
  if (!track || (track.color ?? 0) === color) return
  await dispatch({ type: "updatePlaylistTrack", id, patch: { color } })
}

/** Deletes a track and its clips, asking first when it has any. */
export async function deleteTrack(id: PlaylistTrackId): Promise<void> {
  const track = playlist().tracks.find((item) => item.id === id)
  if (!track) return
  const count = playlist().clips.filter((clip) => clip.track === id).length
  if (count > 0) {
    const choice = await askConfirm({
      title: message("Delete {trackName}?", { trackName: track.name }),
      description: message("Its {shown} clips will be deleted with it. Undo brings them back.", { count, shown: plainNumber(count) }),
      choices: [
        { id: "delete", label: t("Delete track"), variant: "destructive" },
      ],
    })
    if (choice !== "delete") return
  }
  const removed = await dispatch({ type: "removePlaylistTrack", id })
  if (removed && ui().targetTrack === id) ui().setTargetTrack(null)
}

/**
 * Moves the song position. The engine has one playhead that restarts when
 * the mode changes, so in pattern mode this only remembers the place; it is
 * sent to the engine when the song is switched on from the playlist.
 */
export async function seekSong(tick: number): Promise<void> {
  const target = Math.max(0, tick)
  ui().setCursorTick(target)
  if (useTransportStore.getState().mode === "song") await seek(target)
}

// True while this file is moving the engine to the song cursor, so the
// panel's own watcher of the mode does not do it a second time.
let applyingCursor = false

/**
 * Sends the place set on the ruler to the engine. The engine starts a mode
 * from the top, so this runs whenever song mode is switched on.
 */
export async function applySongCursor(): Promise<void> {
  const cursor = ui().cursorTick
  if (applyingCursor || cursor <= 0) return
  applyingCursor = true
  try {
    await seek(cursor)
  } finally {
    applyingCursor = false
  }
}

/** Switches the transport to the song, at the place set on the ruler. */
export async function enterSongMode(): Promise<void> {
  if (useTransportStore.getState().mode === "song") return
  const cursor = ui().cursorTick
  applyingCursor = true
  try {
    await setPlayMode("song")
    if (cursor > 0) await seek(cursor)
  } finally {
    applyingCursor = false
  }
}

/** One click from pattern mode to hearing the song. */
export async function playSong(): Promise<void> {
  await enterSongMode()
  if (!useTransportStore.getState().playing) await play()
}

export function toggleLoopSong(): Promise<void> {
  return setTransport({ loopSong: !useTransportStore.getState().loopSong })
}
