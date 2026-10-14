import type { Command, MixerTrack, MixerTrackPatch, TrackId } from "@/bindings"
import {
  askConfirm,
  dispatch,
  newGestureId,
  useProjectStore,
  useUiStore,
} from "@/lib/store"
import { clamp, MASTER_TRACK, MAX_GAIN, MAX_MIXER_TRACKS } from "@/lib/units"

import { automationGoingWith } from "@/features/automation/owned"

import { effectName } from "./effect-ops"
import { useMixerUi, selectedMixerTracks, visualMixerOrder, selectMixerTrack } from "./mixer-ui"
import { resetAllPeaks } from "./peaks"
import { feedersOf } from "./routing"
import { message, plainNumber, translatedText as t, historyLabel } from "@/lib/i18n"

/*
 * Everything the mixer does to the project, in one place. The strips and
 * the registry actions both call these, so a click and a palette entry do
 * the same thing.
 */

const project = () => useProjectStore.getState().project
const ui = () => useUiStore.getState()

export function mixerTracks(): MixerTrack[] {
  return project().mixer.tracks
}

export function findTrack(id: TrackId | null): MixerTrack | undefined {
  return mixerTracks().find((track) => track.id === id)
}

/** The selected track, or undefined when none is or it no longer exists. */
export function selectedTrack(): MixerTrack | undefined {
  return findTrack(ui().selectedTrack)
}

export function mixerIsFull(): boolean {
  return mixerTracks().filter((track) => !track.current).length >= MAX_MIXER_TRACKS
}

/**
 * The backend clamps values that are out of range but the browser mock
 * rejects them, so everything is brought into range before it is sent.
 */
export function clampGain(gain: number): number {
  return Number.isFinite(gain) ? clamp(gain, 0, MAX_GAIN) : 0
}

export function clampPan(pan: number): number {
  return Number.isFinite(pan) ? clamp(pan, -1, 1) : 0
}

export function patchTrack(id: TrackId, patch: MixerTrackPatch) {
  if (patch.muted !== undefined || patch.solo !== undefined) {
    const tracks = selectedMixerTracks(id).filter((track) => patch.solo === undefined || track.id !== MASTER_TRACK && !track.current)
    return dispatch({ type: "batch", commands: tracks.map((track) => ({ type: "updateMixerTrack", id: track.id, patch })) })
  }
  return dispatch({ type: "updateMixerTrack", id, patch })
}

export async function addTrack(): Promise<void> {
  const result = await dispatch({ type: "addMixerTrack" })
  if (!result) return
  ui().setPanelVisible("mixer", true)
  ui().selectTrack(result.created[0])
}

function listNames(names: string[]): string {
  const shown = names.slice(0, 3).map((name) => `"${name}"`)
  const rest = names.length - shown.length
  return rest > 0 ? message("{names} and {rest} more", { names: shown.join(", "), rest: plainNumber(rest) }) : shown.join(", ")
}

/**
 * What deleting a track takes with it and undoes for everything that plays
 * into it.
 */
export function deleteWarning(track: MixerTrack): string | null {
  const { channels, clips, outputs, sends } = feedersOf(
    mixerTracks(),
    project().channels,
    project().playlist.clips,
    track.id
  )
  const parts: string[] = []
  const keys = mixerTracks().filter((source) => source.sidechains?.some((send) => send.target === track.id))
  if (keys.length) parts.push(message("Detector-only routes from {names} will be removed.", { names: listNames(keys.map((source) => source.name)) }))
  if (channels.length === 1) {
    parts.push(
      message("The channel \"{name}\" plays into this track. It will fall back to the master.", { name: channels[0].name })
    )
  } else if (channels.length > 1) {
    parts.push(
      message("{count} channels play into this track: {names}. They will fall back to the master.", { count: channels.length, names: listNames(channels.map((channel) => channel.name)) })
    )
  }
  if (clips.length === 1) {
    parts.push(
      t("An audio clip on the playlist plays into this track. It will play into the master instead.")
    )
  } else if (clips.length > 1) {
    parts.push(
      message("{count} audio clips on the playlist play into this track. They will play into the master instead.", { count: clips.length })
    )
  }
  if (outputs.length === 1) {
    parts.push(
      message("The track \"{name}\" is routed into it and will go to the master instead.", { name: outputs[0].name })
    )
  } else if (outputs.length > 1) {
    parts.push(
      message("{count} tracks are routed into it and will go to the master instead.", { count: outputs.length })
    )
  }
  if (sends.length > 0) {
    parts.push(
      sends.length === 1
        ? message("The send from \"{name}\" will be removed.", { name: sends[0].name })
        : message("Sends from {count} tracks will be removed.", { count: sends.length })
    )
  }
  const effects = track.effects.length
  if (effects === 1) {
    parts.push(message("Its {effect} goes with it.", { effect: effectName(track.effects[0].params.type) }))
  } else if (effects > 1) {
    parts.push(message("Its {count} effects go with it.", { count: effects }))
  }
  // Of its fader and pan, of its effects, and of the sends from and to it.
  const automation = automationGoingWith({ type: "track", track: track.id })
  if (automation !== null) parts.push(automation)
  return parts.length > 0 ? parts.join(" ") : null
}

/**
 * Deletes a track. Asks first when it has effects, when anything plays
 * into it, or when automation goes with it.
 */
export async function deleteTrack(id: TrackId): Promise<void> {
  const track = findTrack(id)
  if (!track || id === MASTER_TRACK) return
  const warning = deleteWarning(track)
  if (warning !== null) {
    const choice = await askConfirm({
      title: message("Delete the track \"{trackName}\"?", { trackName: track.name }),
      description: warning,
      choices: [
        { id: "delete", label: t("Delete track"), variant: "destructive" },
      ],
    })
    if (choice !== "delete") return
  }
  const tracks = mixerTracks()
  const index = tracks.findIndex((item) => item.id === id)
  const neighbor = tracks[index + 1] ?? tracks[index - 1]
  const wasSelected = ui().selectedTrack === id
  const result = await dispatch({ type: "removeMixerTrack", id })
  if (result && wasSelected) ui().selectTrack(neighbor?.id ?? MASTER_TRACK)
}

/** Opens the name of a track for editing in its strip. */
export function startRename(id: TrackId) {
  if (!findTrack(id)) return
  ui().setPanelVisible("mixer", true)
  ui().selectTrack(id)
  useMixerUi.setState({ renaming: id, coloring: null })
}

export async function renameTrack(id: TrackId, name: string): Promise<void> {
  const track = findTrack(id)
  const trimmed = name.trim()
  if (!track || trimmed === "" || trimmed === track.name) return
  await patchTrack(id, { name: trimmed })
}

/** Opens the color swatches of a track. */
export function startColoring(id: TrackId) {
  if (!findTrack(id)) return
  ui().setPanelVisible("mixer", true)
  ui().selectTrack(id)
  useMixerUi.setState({ coloring: id, renaming: null })
}

export async function setTrackColor(id: TrackId, color: number) {
  if (findTrack(id)?.color === color) return
  await patchTrack(id, { color: color & 0xffffff })
}

export async function resetVolume(id: TrackId) {
  if (findTrack(id)?.volume !== 1) await patchTrack(id, { volume: 1 })
}

export async function centerPan(id: TrackId) {
  if (findTrack(id)?.pan !== 0) await patchTrack(id, { pan: 0 })
}

export async function toggleMute(id: TrackId) {
  const track = findTrack(id)
  if (track) await patchTrack(id, { muted: !track.muted })
}

export async function toggleSolo(id: TrackId) {
  const track = findTrack(id)
  if (track && !track.current) await patchTrack(id, { solo: !track.solo })
}

/** Applies one patch to several tracks as a single undo step. */
async function patchTracks(tracks: MixerTrack[], patch: MixerTrackPatch) {
  const gesture = newGestureId()
  for (const track of tracks) {
    await dispatch({ type: "updateMixerTrack", id: track.id, patch }, gesture)
  }
}

export function unmuteAll() {
  return patchTracks(
    mixerTracks().filter((track) => track.muted),
    { muted: false }
  )
}

/** Returns every fader and pan to unity as a single undo step. */
export async function resetLevels(): Promise<void> {
  const commands: Command[] = []
  for (const track of mixerTracks()) {
    const patch: MixerTrackPatch = {}
    if (track.volume !== 1) patch.volume = 1
    if (track.pan !== 0) patch.pan = 0
    if (patch.volume === undefined && patch.pan === undefined) continue
    commands.push({ type: "updateMixerTrack", id: track.id, patch })
  }
  if (commands.length === 0) return
  await dispatch({ type: "batch", label: historyLabel("Reset mixer levels"), commands })
}

export function unsoloAll() {
  return patchTracks(
    mixerTracks().filter((track) => track.solo),
    { solo: false }
  )
}

/** Routes a track's output to another track, or nowhere with `null`. */
export async function setOutput(id: TrackId, output: TrackId | null) {
  const track = findTrack(id)
  if (!track || track.current || id === MASTER_TRACK || track.output === output) return
  await dispatch({
    type: "setTrackOutput",
    id,
    output: output ?? undefined,
  })
}

export async function removeSidechain(from: TrackId, to: TrackId) {
  const automation = automationGoingWith({ type: "sidechain", track: from, target: to })
  if (automation !== null && await askConfirm({ title: t("Remove sidechain?"), description: message("{automation} Undo brings it back.", { automation }), choices: [{ id: "remove", label: t("Remove sidechain"), variant: "destructive" }] }) !== "remove") return
  await dispatch({ type: "setSidechain", from, to })
}

export function addSend(from: TrackId, to: TrackId) {
  return dispatch({ type: "setSend", from, to, gain: 1 })
}

/**
 * Removes a send. Asks first only when automation of its level would be
 * deleted with it; a send nothing automates goes at once.
 */
export async function removeSend(from: TrackId, to: TrackId): Promise<void> {
  const automation = automationGoingWith({
    type: "send",
    track: from,
    target: to,
  })
  if (automation !== null) {
    const target = findTrack(to)?.name ?? t("the track")
    const choice = await askConfirm({
      title: message("Remove the send to \"{target}\"?", { target }),
      description: message("{automation} Undo brings them back.", { automation }),
      choices: [{ id: "remove", label: t("Remove send"), variant: "destructive" }],
    })
    if (choice !== "remove") return
  }
  await dispatch({ type: "setSend", from, to })
}

/** Moves the selection one strip left (-1) or right (1), or to an end. */
export function selectStrip(move: -1 | 1 | "first" | "last"): TrackId | null {
  const tracks = visualMixerOrder(mixerTracks())
  if (tracks.length === 0) return null
  const current = tracks.findIndex((track) => track.id === ui().selectedTrack)
  let next: number
  if (move === "first") next = 0
  else if (move === "last") next = tracks.length - 1
  else if (current < 0) next = move === 1 ? 0 : tracks.length - 1
  else next = clamp(current + move, 0, tracks.length - 1)
  const id = tracks[next].id
  ui().selectTrack(id)
  return id
}

/** Moves the selection as a key does: the strip it lands on takes the focus. */
export function moveSelection(move: -1 | 1 | "first" | "last", range = false) {
  if (range) {
    const tracks = visualMixerOrder(mixerTracks())
    if (!tracks.length) return
    const index = tracks.findIndex((track) => track.id === ui().selectedTrack)
    const next = move === "first" ? 0 : move === "last" ? tracks.length - 1 : clamp(index + move, 0, tracks.length - 1)
    const id = tracks[next].id
    selectMixerTrack(id, false, true)
    useMixerUi.setState({ focusing: id })
    return
  }
  const id = selectStrip(move)
  if (id !== null) useMixerUi.setState({ focusing: id })
}

export { resetAllPeaks }
