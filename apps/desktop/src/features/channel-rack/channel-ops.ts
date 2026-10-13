import { openPresetRender } from "@/features/preset-render/dialog"
import { isPresetPath } from "@/features/preset-render/types"
import type {
  Channel,
  ChannelId,
  ChannelPatch,
  Command,
  InstrumentKind,
  InstrumentParams,
  LibraryFileToken,
  Note,
  PatternId,
  SampleId,
  TrackId,
} from "@/bindings"
import { automationGoingWith } from "@/features/automation/owned"
import { instrumentDescriptor } from "@/features/params"
import { instrumentParams, sourceSample } from "@/lib/channel-source"
import { attempt } from "@/lib/errors"
import { currentPatternId } from "@/lib/flows/edit"
import { backend } from "@/lib/ipc"
import { newGestureId } from "@/lib/store/gesture"
import { dispatch, receivePatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { askConfirm, askText } from "@/lib/store/prompts"
import { useUiStore } from "@/lib/store/ui"
import { clamp } from "@/lib/units"

import { useRackStore } from "./rack-store"
import { clampPatternLength, fillCommand, shiftCommand } from "./steps"
import { translatedText as t, message } from "@/lib/i18n"

/*
 * Everything the rack does to a channel, in one place. Rows, menus, the
 * inspector and the registry actions all call these, so a row's right-click
 * menu and the command palette can never disagree.
 */

const project = () => useProjectStore.getState().project

export function findChannel(id: ChannelId | null): Channel | undefined {
  return project().channels.find((channel) => channel.id === id)
}

/** The selected channel, or undefined when it is gone or nothing is selected. */
export function selectedChannel(): Channel | undefined {
  return findChannel(useUiStore.getState().selectedChannel)
}

export function selectChannel(
  id: ChannelId,
  options: { openSettings?: boolean } = {}
) {
  useUiStore.getState().selectChannel(id)
  if (options.openSettings) useRackStore.getState().setInspectorOpen(true)
}

function patternContext(channel: ChannelId): {
  pattern: PatternId
  lengthSteps: number
  notes: readonly Note[] | undefined
} | null {
  const id = currentPatternId()
  const pattern = project().patterns.find((item) => item.id === id)
  if (!pattern) return null
  return {
    pattern: pattern.id,
    lengthSteps: pattern.lengthSteps,
    notes: pattern.lanes.find((lane) => lane.channel === channel)?.notes,
  }
}

export async function renameChannel(id: ChannelId): Promise<void> {
  const channel = findChannel(id)
  if (!channel) return
  const name = await askText({
    title: t("Rename channel"),
    label: t("Name"),
    initial: channel.name,
    submitLabel: t("Rename"),
  })
  if (name === null || name === channel.name) return
  await dispatch({ type: "updateChannel", id, patch: { name } })
}

export async function deleteChannel(id: ChannelId): Promise<void> {
  const channel = findChannel(id)
  if (!channel) return
  const automation = automationGoingWith({ type: "channel", channel: id })
  const choice = await askConfirm({
    title: message("Delete {channelName}?", { channelName: channel.name }),
    description: [
      t("Its steps in every pattern are deleted with it. Its mixer track stays."),
      automation,
      t("Undo brings the channel back."),
    ]
      .filter((part) => part !== null)
      .join(" "),
    choices: [
      { id: "delete", label: t("Delete channel"), variant: "destructive" },
    ],
  })
  if (choice !== "delete") return
  const channels = project().channels
  const index = channels.findIndex((item) => item.id === id)
  const neighbor = channels[index + 1] ?? channels[index - 1]
  const result = await dispatch({ type: "removeChannel", id })
  if (result && useUiStore.getState().selectedChannel === id) {
    useUiStore.getState().selectChannel(neighbor?.id ?? null)
  }
}

export async function duplicateChannel(id: ChannelId): Promise<void> {
  const result = await dispatch({ type: "duplicateChannel", id })
  if (result) selectChannel(result.created[0])
}

/** Moves a channel to a position in the rack. Positions past the end mean the end. */
export async function moveChannelTo(
  id: ChannelId,
  index: number
): Promise<void> {
  const channels = project().channels
  const from = channels.findIndex((channel) => channel.id === id)
  const to = clamp(index, 0, channels.length - 1)
  if (from < 0 || from === to) return
  await dispatch({ type: "moveChannel", id, index: to })
}

export function moveChannelBy(id: ChannelId, delta: number): Promise<void> {
  const from = project().channels.findIndex((channel) => channel.id === id)
  return from < 0 ? Promise.resolve() : moveChannelTo(id, from + delta)
}

export async function toggleMute(id: ChannelId): Promise<void> {
  const channel = findChannel(id)
  if (!channel) return
  await dispatch({
    type: "updateChannel",
    id,
    patch: { muted: !channel.muted },
  })
}

/**
 * Solo means "hear only this one": turning it on for a channel turns it off
 * for the others, as one undo step. Turning it off touches only this channel.
 */
export async function toggleSolo(id: ChannelId): Promise<void> {
  const channel = findChannel(id)
  if (!channel) return
  if (channel.solo) {
    await dispatch({ type: "updateChannel", id, patch: { solo: false } })
    return
  }
  const commands: Command[] = [
    { type: "updateChannel", id, patch: { solo: true } },
    ...project()
      .channels.filter((other) => other.solo && other.id !== id)
      .map((other): Command => ({
        type: "updateChannel",
        id: other.id,
        patch: { solo: false },
      })),
  ]
  await dispatch(
    commands.length === 1
      ? commands[0]
      : { type: "batch", label: t("Solo channel"), commands }
  )
}

export async function unmuteAllChannels(): Promise<void> {
  const commands: Command[] = project()
    .channels.filter((channel) => channel.muted)
    .map((channel) => ({
      type: "updateChannel",
      id: channel.id,
      patch: { muted: false },
    }))
  if (commands.length === 0) return
  await dispatch({ type: "batch", label: t("Unmute channels"), commands })
}

export async function unsoloAllChannels(): Promise<void> {
  const commands: Command[] = project()
    .channels.filter((channel) => channel.solo)
    .map((channel) => ({
      type: "updateChannel",
      id: channel.id,
      patch: { solo: false },
    }))
  if (commands.length === 0) return
  await dispatch({ type: "batch", label: t("Unsolo channels"), commands })
}

/** Returns every channel's volume and pan to unity as a single undo step. */
export async function resetChannelLevels(): Promise<void> {
  const commands: Command[] = []
  for (const channel of project().channels) {
    const patch: ChannelPatch = {}
    if (channel.volume !== 1) patch.volume = 1
    if (channel.pan !== 0) patch.pan = 0
    if (patch.volume === undefined && patch.pan === undefined) continue
    commands.push({ type: "updateChannel", id: channel.id, patch })
  }
  if (commands.length === 0) return
  await dispatch({ type: "batch", label: t("Reset channel levels"), commands })
}

export async function setChannelColor(
  id: ChannelId,
  color: number
): Promise<void> {
  const channel = findChannel(id)
  if (!channel || channel.color === color) return
  await dispatch({
    type: "updateChannel",
    id,
    patch: { color: color & 0xffffff },
  })
}

/** True when the channel has notes in the pattern being edited. */
export function hasNotes(id: ChannelId): boolean {
  return (patternContext(id)?.notes?.length ?? 0) > 0
}

export async function clearSteps(id: ChannelId): Promise<void> {
  const context = patternContext(id)
  if (!context?.notes?.length) return
  await dispatch({
    type: "clearLane",
    pattern: context.pattern,
    channel: id,
  })
}

export async function fillEvery(id: ChannelId, every: number): Promise<void> {
  const context = patternContext(id)
  if (!context) return
  await dispatch(
    fillCommand(context.pattern, id, context.notes, context.lengthSteps, every)
  )
}

export async function shiftSteps(id: ChannelId, by: -1 | 1): Promise<void> {
  const context = patternContext(id)
  if (!context) return
  const command = shiftCommand(
    context.pattern,
    id,
    context.notes,
    context.lengthSteps,
    by
  )
  if (command) await dispatch(command)
}

export async function routeChannel(
  id: ChannelId,
  track: TrackId
): Promise<void> {
  const channel = findChannel(id)
  if (!channel) return
  if (channel.mixerTrack !== track) {
    const result = await dispatch({
      type: "updateChannel",
      id,
      patch: { mixerTrack: track },
    })
    if (!result) return
  }
  useUiStore.getState().selectTrack(track)
}

/** Adds a mixer track named after the channel and routes the channel to it. */
export async function routeToNewTrack(id: ChannelId): Promise<void> {
  const channel = findChannel(id)
  if (!channel) return
  // Both edits share a gesture id, so undo takes them back together.
  const gesture = newGestureId()
  const added = await dispatch(
    { type: "addMixerTrack", name: channel.name },
    gesture
  )
  if (!added) return
  const track = added.created[0]
  const routed = await dispatch(
    { type: "updateChannel", id, patch: { mixerTrack: track } },
    gesture
  )
  if (routed) useUiStore.getState().selectTrack(track)
}

/** Selects the channel's mixer track and makes sure the mixer is showing. */
export function showInMixer(id: ChannelId) {
  const channel = findChannel(id)
  if (!channel) return
  const ui = useUiStore.getState()
  ui.selectTrack(channel.mixerTrack)
  ui.setPanelVisible("mixer", true)
}

/** Adds a channel that plays an audio file, at a position in the rack. */
export async function addChannelFromFile(
  path: string,
  index?: number,
  browser?: LibraryFileToken
): Promise<void> {
  const generation = getProjectGeneration()
  const count = project().channels.length
  const at = index === undefined ? undefined : clamp(index, 0, count)
  const result = await attempt(
    isPresetPath(path)
      ? openPresetRender({ path, index: at, browser }, undefined, () => generation === getProjectGeneration())
      : browser
      ? backend.addChannelFromFile(path, at, browser)
      : backend.addChannelFromFile(path, at),
    t("Could not add the sample")
  )
  if (!result || generation !== getProjectGeneration()) return
  receivePatch(result.patch)
  const created = result.patch.channels?.find((channel) =>
    result.created.includes(channel.id)
  )
  if (created) selectChannel(created.id)
}

/** Adds a channel that plays a built-in instrument and opens its settings. */
export async function addInstrumentChannel(
  kind: InstrumentKind
): Promise<void> {
  const result = await dispatch({ type: "addChannel", instrument: kind })
  if (!result) return
  selectChannel(result.created[0], { openSettings: true })
  useUiStore.getState().showCenterTab("channelRack")
}

/**
 * Replaces every setting of the channel's instrument, as one undo step
 * that the history names after what was loaded.
 */
export async function loadInstrumentSound(
  id: ChannelId,
  params: InstrumentParams,
  label: string
): Promise<void> {
  await dispatch({
    type: "batch",
    label,
    commands: [{ type: "setInstrumentParams", channel: id, params }],
  })
}

/** Puts the channel's instrument back to the settings a new one has. */
export async function initInstrument(id: ChannelId): Promise<void> {
  const params = instrumentParams(findChannel(id))
  if (!params) return
  const descriptor = instrumentDescriptor(params.type)
  await loadInstrumentSound(
    id,
    descriptor.defaults,
    `Init ${descriptor.name.toLowerCase()}`
  )
}

/** Asks for an audio file and adds a channel that plays it. */
export async function addChannelFromPickedFile(): Promise<void> {
  const generation = getProjectGeneration()
  const path = await attempt(backend.pickAudioFile(), t("Could not choose a file"))
  if (path === null || generation !== getProjectGeneration()) return
  await addChannelFromFile(path)
  if (generation === getProjectGeneration())
    useUiStore.getState().showCenterTab("channelRack")
}

/** Asks for an audio file and makes the channel play it instead. */
export async function replaceSampleFromPickedFile(
  id: ChannelId
): Promise<void> {
  const generation = getProjectGeneration()
  const path = await attempt(backend.pickAudioFile(), t("Could not choose a file"))
  if (path !== null && generation === getProjectGeneration())
    await replaceSampleFromFile(id, path)
}

export async function replaceSampleFromFile(
  id: ChannelId,
  path: string,
  browser?: LibraryFileToken
): Promise<void> {
  const generation = getProjectGeneration()
  const result = await attempt(
    isPresetPath(path)
      ? openPresetRender({ path, channel: id, browser }, undefined, () => generation === getProjectGeneration())
      : browser
      ? backend.setChannelSampleFromFile(id, path, browser)
      : backend.setChannelSampleFromFile(id, path),
    t("Could not change the sample")
  )
  if (result && generation === getProjectGeneration())
    receivePatch(result.patch)
}

/** Points the channel at a sample that is already in the project. */
export async function assignProjectSample(
  id: ChannelId,
  sample: SampleId
): Promise<void> {
  const channel = findChannel(id)
  if (!channel || sourceSample(channel.source) === sample) return
  await dispatch({ type: "setChannelSample", id, sample })
}

export async function setPatternLength(steps: number): Promise<void> {
  const id = currentPatternId()
  const pattern = project().patterns.find((item) => item.id === id)
  if (!pattern) return
  const lengthSteps = clampPatternLength(steps)
  if (lengthSteps === pattern.lengthSteps) return
  await dispatch({
    type: "updatePattern",
    id: pattern.id,
    patch: { lengthSteps },
  })
}
