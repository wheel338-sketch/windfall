import { create } from "zustand"

import type { Channel, ChannelId, Command } from "@/bindings"
import type { ContextItem } from "@/components/context-actions"
import { dispatch, onHistoryNavigation, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { askConfirm, askText } from "@/lib/store/prompts"
import { useUiStore } from "@/lib/store/ui"

import { useRackStore } from "./rack-store"
import { translatedText as t, message, dynamicTitle } from "@/lib/i18n"

export function channelGroups(channels: readonly Channel[]): string[] {
  return [...new Set(channels.map((channel) => channel.group ?? "").filter(Boolean))]
    .sort((a, b) => a.localeCompare(b))
}

export function validGroupName(name: string): boolean {
  return new TextEncoder().encode(name.trim()).length <= 128 && !Array.from(name.trim()).some((character) => {
    const code = character.codePointAt(0)!
    return code <= 31 || (code >= 127 && code <= 159)
  })
}

export type GroupRequest = {
  id: number
  generation: number
  revision: number
  channels: Channel[]
  selected: ChannelId | null
  filter: string | null
}
let nextRequest = 0
export const useChannelGroups = create<{ request: GroupRequest | null }>(() => ({ request: null }))

export function openChannelGroups(): void {
  const { project, revision } = useProjectStore.getState()
  useUiStore.getState().showCenterTab("channelRack")
  useChannelGroups.setState({ request: {
    id: ++nextRequest, generation: getProjectGeneration(), revision,
    channels: project.channels.map((channel) => ({ ...channel })),
    selected: useUiStore.getState().selectedChannel,
    filter: useRackStore.getState().groupFilter,
  } })
}

export function closeChannelGroups(): void {
  useChannelGroups.setState({ request: null })
}

export function groupRequestIsCurrent(request: GroupRequest): boolean {
  return useChannelGroups.getState().request === request &&
    getProjectGeneration() === request.generation && useProjectStore.getState().revision === request.revision
}

export async function assignChannelGroups(request: GroupRequest, channels: ChannelId[], group: string): Promise<boolean> {
  if (!groupRequestIsCurrent(request) || !validGroupName(group) || channels.length === 0) return false
  if (!channels.every((id) => request.channels.some((channel) => channel.id === id))) return false
  const result = await dispatch({ type: "setChannelGroup", channels, group: group.trim() })
  if (!result) return false
  if (getProjectGeneration() === request.generation) useRackStore.getState().setGroupFilter(group.trim())
  if (useChannelGroups.getState().request === request) closeChannelGroups()
  return true
}

export async function createChannelGroup(): Promise<void> {
  const state = useProjectStore.getState()
  const id = useUiStore.getState().selectedChannel
  if (id === null || !state.project.channels.some((channel) => channel.id === id)) {
    openChannelGroups()
    return
  }
  const generation = getProjectGeneration()
  const name = await askText({ title: t("Create channel group"), label: t("Group name"), initial: "", submitLabel: t("Create and assign") })
  if (name === null || !name.trim() || !validGroupName(name) || generation !== getProjectGeneration()) return
  if (!useProjectStore.getState().project.channels.some((channel) => channel.id === id)) return
  const result = await dispatch({ type: "setChannelGroup", channels: [id], group: name.trim() })
  if (result && generation === getProjectGeneration()) useRackStore.getState().setGroupFilter(name.trim())
}

export function currentNamedGroup(): string | null {
  const filter = useRackStore.getState().groupFilter
  if (filter) return filter
  return useProjectStore.getState().project.channels.find((channel) => channel.id === useUiStore.getState().selectedChannel)?.group || null
}

export async function renameChannelGroup(): Promise<void> {
  const name = currentNamedGroup()
  if (!name) return
  const generation = getProjectGeneration()
  const newName = await askText({ title: message("Rename group: {name}", { name }), label: t("Group name"), initial: name, submitLabel: t("Rename") })
  if (newName === null || !newName.trim() || !validGroupName(newName) || generation !== getProjectGeneration()) return
  const result = await dispatch({ type: "renameChannelGroup", name, newName: newName.trim() })
  if (result && generation === getProjectGeneration()) useRackStore.getState().setGroupFilter(newName.trim())
}

export async function removeChannelGroup(): Promise<void> {
  const name = currentNamedGroup()
  if (!name) return
  const generation = getProjectGeneration()
  const answer = await askConfirm({
    title: message("Remove group: {name}?", { name }),
    description: t("Its channels become ungrouped. All channels, notes and mixer routing stay. Undo restores the group."),
    choices: [{ id: "remove", label: t("Remove group") }],
  })
  if (answer !== "remove" || generation !== getProjectGeneration()) return
  await dispatch({ type: "removeChannelGroup", name })
}

/** Group submenu captures the document and clicked channel. */
export function channelGroupItems(channel: ChannelId): ContextItem[] {
  const generation = getProjectGeneration()
  const project = useProjectStore.getState().project
  const current = project.channels.find((item) => item.id === channel)?.group ?? ""
  const assign = async (group: string) => {
    if (generation !== getProjectGeneration()) return
    const command: Command = { type: "setChannelGroup", channels: [channel], group }
    const result = await dispatch(command)
    if (result && generation === getProjectGeneration()) {
      const filter = useRackStore.getState().groupFilter
      if (filter !== null) useRackStore.getState().setGroupFilter(group)
    }
  }
  return [
    { ...dynamicTitle("Ungrouped", {}), checked: current === "", run: () => assign("") },
    ...channelGroups(project.channels).map((group) => ({ title: group, checked: group === current, run: () => assign(group) })),
    { separator: true }, "channelRack.createGroup", "channelRack.groups",
  ]
}

onProjectReplaced(closeChannelGroups)
onHistoryNavigation(closeChannelGroups)
