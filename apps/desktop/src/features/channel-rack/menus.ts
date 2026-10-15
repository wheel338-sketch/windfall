import type { AutomationTarget, ChannelId } from "@/bindings"
import {
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import { automationItems } from "@/features/automation/menu"
import { INSTRUMENT_KINDS } from "@/features/params"

import { LENGTH_PRESETS } from "./actions"
import { showInMixer } from "./channel-ops"
import { translatedText as t, dynamicTitle } from "@/lib/i18n"

/*
 * The right-click menus of the rack's empty areas. A channel's name has a
 * menu of its own, about that channel; these are about the rack and the
 * pattern being edited.
 */

const ADD_CHANNEL: ContextItem = {
  get submenu() { return t("Add channel") },
  items: [
    "channel.add",
    ...INSTRUMENT_KINDS.map((kind) => `channel.addInstrument.${kind}`),
    "channel.addGeneralMidi",
    "channel.addLayer",
    "channel.addFromFile",
  ],
}

const PATTERN_LENGTH: ContextItem = {
  get submenu() { return t("Pattern length") },
  items: LENGTH_PRESETS.map((steps) => `pattern.length${steps}`),
}

/** Anywhere in the rack that is not a control with a menu of its own. */
export const RACK_MENU: ContextItem[] = [
  ADD_CHANNEL,
  PATTERN_LENGTH,
  contextSeparator,
  "pattern.add",
  "pattern.duplicate",
  contextSeparator,
  "channel.unmuteAll",
  "channel.unsoloAll",
  "channel.resetLevels",
  "channel.selectNextMuted",
  "channel.selectNextSolo",
  contextSeparator,
  "channelRack.settings",
  "channelRack.graph",
  "channelRack.groups",
  "channelRack.createGroup",
  "channelRack.renameGroup",
  "channelRack.removeGroup",
  "channelRack.showAllGroups",
]

/** The settings of the selected channel, outside its controls. */
export const INSPECTOR_MENU: ContextItem[] = [
  "channel.rename",
  "channel.color",
  "channel.replaceSample",
  "channel.initInstrument",
  contextSeparator,
  "channel.showInMixer",
  "channelRack.settings",
]

/** What automation calls a channel's volume or pan. */
export function channelTarget(
  channel: ChannelId,
  field: "volume" | "pan"
): AutomationTarget {
  return field === "volume"
    ? { type: "channelVolume", channel }
    : { type: "channelPan", channel }
}

/**
 * What a channel's volume or pan knob in its row offers besides what every
 * value control does. The knob is bound to a setting of the channel, so
 * this is where the entries about that setting go: the ones that make an
 * automation clip for it and lead to the ones it has.
 */
export function channelValueItems(
  channel: ChannelId,
  field: "volume" | "pan"
): ContextItem[] {
  return [
    ...automationItems(channelTarget(channel, field)),
    contextSeparator,
    {
      ...dynamicTitle(field === "volume"
          ? "Show the mixer track it plays into"
          : "Show the mixer track it is panned into", {}),
      run: () => showInMixer(channel),
    },
  ]
}
