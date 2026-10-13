import {
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import type { AutomationTarget, TrackId } from "@/bindings"
import { automationItems } from "@/features/automation/menu"
import { EFFECT_KINDS } from "@/features/params"

import { addEffectActionId } from "./effect-actions"
import { resetPeak } from "./peaks"
import { translatedText as t, dynamicTitle } from "@/lib/i18n"

/*
 * The right-click menus of the mixer's empty areas. A strip's header, an
 * effect's slot and an effect's panel have menus of their own, about that
 * one track or effect; these are about the mixer and the selected track.
 */

const ADD_EFFECT: ContextItem = {
  get submenu() { return t("Add effect") },
  items: EFFECT_KINDS.map(addEffectActionId),
}

/** Anywhere in the mixer that is not a control with a menu of its own. */
export const MIXER_MENU: ContextItem[] = [
  "mixer.renderSelected",
  "mixer.renderArmed",
  contextSeparator,
  "mixer.addTrack",
  ADD_EFFECT,
  contextSeparator,
  "mixer.effects",
  "mixer.bypassEffects",
  "mixer.enableEffects",
  "mixer.enlargeEffects",
  contextSeparator,
  "mixer.unmuteAll",
  "mixer.resetLevels",
  "mixer.selectRoutedHere",
  "mixer.selectMutedTracks",
  "mixer.selectSoloTracks",
  "mixer.unsoloAll",
  "mixer.resetPeaks",
  contextSeparator,
  "view.mixer",
]

/** A value of a track that a control on its strip is bound to. */
export type TrackField = "volume" | "pan" | { send: TrackId } | { sidechain: TrackId }

/** What automation calls the thing a strip's control is bound to. */
export function trackTarget(
  track: TrackId,
  field: TrackField
): AutomationTarget {
  if (field === "volume") return { type: "trackVolume", track }
  if (field === "pan") return { type: "trackPan", track }
  return "sidechain" in field ? { type: "sidechainGain", track, target: field.sidechain } : { type: "sendGain", track, target: field.send }
}

/**
 * What a strip's fader, pan knob or send knob offers besides what every
 * value control does. Each is bound to a setting of the track, so this is
 * where the entries about that setting go: the ones that make an
 * automation clip for it and lead to the ones it has.
 */
export function trackValueItems(
  track: TrackId,
  field: TrackField
): ContextItem[] {
  const automation = automationItems(trackTarget(track, field))
  if (field !== "volume") return automation
  return [
    ...automation,
    contextSeparator,
    { ...dynamicTitle("Reset peak readout", {}), run: () => resetPeak(track) },
  ]
}

/** The effects of the selected track, outside an effect's own header. */
export const INSPECTOR_MENU: ContextItem[] = [
  ADD_EFFECT,
  contextSeparator,
  "mixer.enlargeEffects",
  "mixer.effects",
]
