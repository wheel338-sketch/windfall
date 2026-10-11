import type { Channel, ChannelId, LayerSettings } from "@/bindings"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { MASTER_TRACK } from "@/lib/units"

import { selectChannel } from "./channel-ops"
import {
  createDefaultChannelVoiceSettings,
  layerOf,
  MAX_LAYER_CHILDREN,
} from "./voice"

/*
 * A layer channel makes no sound of its own: every note it is given plays
 * on each of its child channels. The project checks the children, so a
 * layer that would play itself, a missing channel or a loop of layers is
 * refused there and reported like any other failed edit.
 */

/** Whether the channel is a layer that is switched on. */
export function isLayerChannel(channel: Channel | undefined): boolean {
  return layerOf(channel?.voice).enabled
}

/** Replaces the channel's layer settings as one undo step. */
export function setLayer(channel: Channel, layer: LayerSettings) {
  return dispatch({
    type: "setChannelVoiceSettings",
    id: channel.id,
    settings: {
      ...(channel.voice ?? createDefaultChannelVoiceSettings()),
      layer,
    },
  })
}

/** The children with `child` added at the end or taken out. */
export function withLayerChild(
  children: readonly ChannelId[],
  child: ChannelId,
  included: boolean
): ChannelId[] {
  const rest = children.filter((id) => id !== child)
  return included ? [...rest, child].slice(0, MAX_LAYER_CHILDREN) : rest
}

/** Adds or removes one child of a layer. */
export function setLayerChild(
  channel: Channel,
  child: ChannelId,
  included: boolean
) {
  const layer = layerOf(channel.voice)
  return setLayer(channel, {
    ...layer,
    children: withLayerChild(layer.children, child, included),
  })
}

/**
 * Adds a layer channel that plays `children` and opens its settings. One
 * undo step. The layer has no mixer track of its own, because it has no
 * sound to mix, so the id of the new channel is the project's next id.
 */
export async function addLayerChannel(
  children: readonly ChannelId[] = []
): Promise<void> {
  const id = useProjectStore.getState().project.nextId
  const result = await dispatch({
    type: "batch",
    label: "Add layer channel",
    commands: [
      { type: "addChannel", name: "Layer", mixerTrack: MASTER_TRACK },
      {
        type: "setChannelVoiceSettings",
        id,
        settings: {
          ...createDefaultChannelVoiceSettings(),
          layer: {
            enabled: true,
            children: children.slice(0, MAX_LAYER_CHILDREN),
          },
        },
      },
    ],
  })
  if (!result) return
  selectChannel(result.created[0], { openSettings: true })
  useUiStore.getState().showCenterTab("channelRack")
}
