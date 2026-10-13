import type {
  EffectId,
  EffectKind,
  EffectSlot,
  MixerTrack,
  TrackId,
} from "@/bindings"
import { automationGoingWith } from "@/features/automation/owned"
import { effectDescriptor } from "@/features/params"
import {
  askConfirm,
  dispatch,
  newGestureId,
  useProjectStore,
  useUiStore,
} from "@/lib/store"
import { clamp } from "@/lib/units"

import { effectEnableUpdates } from "./effect-enable"
import { useEffectsUi } from "./effects-ui"
import { selectedMixerTracks } from "./mixer-ui"
import { message, translatedText as t } from "@/lib/i18n"

/*
 * Everything the mixer does to effects, in one place. The slots on a
 * strip, the inspector and the registry actions all call these, so a drag,
 * a click and a palette entry do the same thing. An effect is found by its
 * id alone: ids are unique in the project, and the track it is on is looked
 * up when the command is sent.
 */

/** Effects one mixer track can hold. Mirrors `MAX_EFFECT_SLOTS`. */
export const MAX_EFFECT_SLOTS = 10

const tracks = () => useProjectStore.getState().project.mixer.tracks
const effectsUi = () => useEffectsUi.getState()

export type FoundEffect = {
  track: MixerTrack
  slot: EffectSlot
  /** The effect's place in its track's chain. */
  index: number
}

export function findEffect(
  id: EffectId | null,
  among: readonly MixerTrack[] = tracks()
): FoundEffect | undefined {
  if (id === null) return undefined
  for (const track of among) {
    const index = track.effects.findIndex((slot) => slot.id === id)
    if (index >= 0) return { track, slot: track.effects[index], index }
  }
  return undefined
}

/** The selected effect, or undefined when none is or it is gone. */
export function selectedEffect(): FoundEffect | undefined {
  return findEffect(effectsUi().selectedEffect)
}

/** How many effects a track has. */
export function chainLength(track: TrackId): number {
  return tracks().find((item) => item.id === track)?.effects.length ?? 0
}

export function trackIsFull(track: MixerTrack | undefined): boolean {
  return (track?.effects.length ?? 0) >= MAX_EFFECT_SLOTS
}

/**
 * Lets go of the selected effect when another track is selected, so the
 * effect actions never act on an effect the inspector is not showing.
 * Returns a function that stops.
 */
export function keepEffectOnSelectedTrack(): () => void {
  return useUiStore.subscribe((state, previous) => {
    if (state.selectedTrack === previous.selectedTrack) return
    const found = selectedEffect()
    if (found && found.track.id !== state.selectedTrack) {
      effectsUi().selectEffect(null)
    }
  })
}

/** Marks an effect as the one the effect actions act on, with its track. */
export function selectEffect(id: EffectId) {
  const found = findEffect(id)
  if (!found) return
  const ui = useUiStore.getState()
  if (ui.selectedTrack !== found.track.id) ui.selectTrack(found.track.id)
  if (effectsUi().selectedEffect !== id) effectsUi().selectEffect(id)
}

/**
 * Shows an effect's editor: selects it and its track, opens the inspector
 * and unfolds and scrolls to the effect there.
 */
export function openEffect(id: EffectId) {
  if (!findEffect(id)) return
  selectEffect(id)
  useUiStore.getState().setPanelVisible("mixer", true)
  effectsUi().setCollapsed(id, false)
  useEffectsUi.setState({ inspectorOpen: true, revealing: id })
}

/** Opens the inspector on a track's chain without picking an effect. */
export function openChain(track: TrackId) {
  const ui = useUiStore.getState()
  if (ui.selectedTrack !== track) ui.selectTrack(track)
  ui.setPanelVisible("mixer", true)
  effectsUi().setInspectorOpen(true)
}

/** Adds an effect at `index`, or at the end, and opens its editor. */
export async function addEffect(
  track: TrackId,
  kind: EffectKind,
  index?: number
): Promise<EffectId | null> {
  const result = await dispatch({ type: "addEffect", track, kind, index })
  if (!result) return null
  const id = result.created[0]
  openEffect(id)
  return id
}

/**
 * Asks before an effect is removed or replaced, but only when automation
 * of it would be deleted with it. An effect nothing automates goes at
 * once: the common case gets no question. Resolves to whether to go on.
 */
async function confirmLosingAutomation(
  id: EffectId,
  kind: EffectKind,
  what: "Remove" | "Replace"
): Promise<boolean> {
  const automation = automationGoingWith({ type: "effect", effect: id })
  if (automation === null) return true
  const choice = await askConfirm({
    title: message(what === "Remove" ? "Remove the {effect}?" : "Replace the {effect}?", { effect: effectName(kind) }),
    description: message("{automation} Undo brings them back.", { automation }),
    choices: [
      {
        id: "go",
        label: what === "Remove" ? t("Remove effect") : t("Replace effect"),
        variant: "destructive",
      },
    ],
  })
  return choice === "go"
}

/**
 * Removes an effect. Its automations and their clips go with it, which it
 * asks about first when there are any.
 */
export async function removeEffect(id: EffectId): Promise<void> {
  const found = findEffect(id)
  if (!found) return
  const { track, index } = found
  if (!(await confirmLosingAutomation(id, found.slot.params.type, "Remove"))) {
    return
  }
  const result = await dispatch({
    type: "removeEffect",
    track: track.id,
    effect: id,
  })
  if (!result || effectsUi().selectedEffect !== id) return
  // The selection moves to the neighbour, so Delete can be pressed again.
  const neighbour = track.effects[index + 1] ?? track.effects[index - 1]
  useEffectsUi.setState({
    selectedEffect: neighbour?.id ?? null,
    focusing: neighbour?.id ?? null,
  })
}

export async function setEffectEnabled(id: EffectId, enabled: boolean) {
  const found = findEffect(id)
  if (!found || found.slot.enabled === enabled) return
  await dispatch({
    type: "updateEffect",
    track: found.track.id,
    effect: id,
    patch: { enabled },
  })
}

export async function toggleEffect(id: EffectId) {
  const found = findEffect(id)
  if (found) await setEffectEnabled(id, !found.slot.enabled)
}

async function setSelectedEffectsEnabled(enabled: boolean): Promise<void> {
  const updates = effectEnableUpdates(selectedMixerTracks(), enabled)
  if (updates.length === 0) return
  await dispatch({
    type: "batch",
    label: enabled ? t("Enable mixer effects") : t("Bypass mixer effects"),
    commands: updates.map(({ track, effect }) => ({
      type: "updateEffect",
      track,
      effect,
      patch: { enabled },
    })),
  })
}

/** Bypasses the selected tracks' effects as one undo step. */
export function bypassSelectedEffects(): Promise<void> {
  return setSelectedEffectsEnabled(false)
}

/** Enables the selected tracks' effects as one undo step. */
export function enableSelectedEffects(): Promise<void> {
  return setSelectedEffectsEnabled(true)
}

/** Copies an effect right after itself and selects the copy. */
export async function duplicateEffect(id: EffectId): Promise<void> {
  const found = findEffect(id)
  if (!found) return
  const result = await dispatch({
    type: "duplicateEffect",
    track: found.track.id,
    effect: id,
  })
  if (result) openEffect(result.created[0])
}

/** Moves an effect one place up (-1) or down (1) in its chain. */
export async function moveEffectBy(id: EffectId, step: -1 | 1): Promise<void> {
  const found = findEffect(id)
  if (!found) return
  const to = clamp(found.index + step, 0, found.track.effects.length - 1)
  if (to === found.index) return
  const result = await dispatch({
    type: "moveEffect",
    track: found.track.id,
    effect: id,
    index: to,
  })
  // Reordering moves the slot's element, which drops the focus it had.
  if (result) useEffectsUi.setState({ focusing: id })
}

/**
 * Where a drop into gap `gap` of `toTrack`'s chain puts an effect, as the
 * index `moveEffect` takes, or null when the drop changes nothing. Gap 0
 * is above the first effect and gap `n` below the last of `n`.
 */
export function moveTarget(
  found: FoundEffect,
  toTrack: TrackId,
  gap: number
): number | null {
  if (found.track.id !== toTrack) return Math.max(0, gap)
  const last = found.track.effects.length - 1
  // The effect leaves its place first, so the gaps below it move up one.
  const to = clamp(gap > found.index ? gap - 1 : gap, 0, last)
  return to === found.index ? null : to
}

/** Whether an effect dragged to `toTrack` can land there at all. */
export function canDrop(id: EffectId, toTrack: TrackId, copy: boolean) {
  const found = findEffect(id)
  const target = tracks().find((track) => track.id === toTrack)
  if (!found || !target) return false
  const sameTrack = found.track.id === toTrack
  return sameTrack && !copy ? true : !trackIsFull(target)
}

/**
 * Puts a copy of an effect into gap `gap` of `toTrack`'s chain as one undo
 * step. The copy is made beside the original and then moved; when the
 * original's track has no room for that, a new effect is added on the other
 * track and given the same settings.
 */
async function copyEffectTo(found: FoundEffect, toTrack: TrackId, gap: number) {
  const { track, slot, index } = found
  const gesture = newGestureId()
  if (!trackIsFull(track)) {
    const copy = await dispatch(
      { type: "duplicateEffect", track: track.id, effect: slot.id },
      gesture
    )
    if (!copy) return null
    const id = copy.created[0]
    const sameTrack = track.id === toTrack
    // The copy sits right below the original, in gap `index + 1`.
    if (!sameTrack || gap !== index + 1) {
      await dispatch(
        {
          type: "moveEffect",
          track: track.id,
          effect: id,
          toTrack: sameTrack ? undefined : toTrack,
          index: gap,
        },
        gesture
      )
    }
    return id
  }
  const added = await dispatch(
    { type: "addEffect", track: toTrack, kind: slot.params.type, index: gap },
    gesture
  )
  if (!added) return null
  const id = added.created[0]
  await dispatch(
    {
      type: "setEffectParams",
      track: toTrack,
      effect: id,
      params: slot.params,
    },
    gesture
  )
  await dispatch(
    {
      type: "updateEffect",
      track: toTrack,
      effect: id,
      patch: { enabled: slot.enabled, mix: slot.mix },
    },
    gesture
  )
  return id
}

/**
 * Finishes a drag: moves the effect into gap `gap` of `toTrack`'s chain,
 * or with `copy` leaves it where it is and puts a copy there.
 */
export async function dropEffect(
  id: EffectId,
  toTrack: TrackId,
  gap: number,
  copy: boolean
): Promise<void> {
  const found = findEffect(id)
  if (!found || !canDrop(id, toTrack, copy)) return
  if (copy) {
    const made = await copyEffectTo(found, toTrack, gap)
    if (made !== null) selectEffect(made)
    return
  }
  const index = moveTarget(found, toTrack, gap)
  if (index === null) return
  const sameTrack = found.track.id === toTrack
  const result = await dispatch({
    type: "moveEffect",
    track: found.track.id,
    effect: id,
    toTrack: sameTrack ? undefined : toTrack,
    index,
  })
  if (result) selectEffect(id)
}

/**
 * Swaps an effect for a new one of another kind in the same place, as one
 * undo step. The new effect starts from its defaults and has an id of its
 * own; the automations of the old one go with it, which it asks about
 * first when there are any.
 */
export async function replaceEffect(id: EffectId, kind: EffectKind) {
  const found = findEffect(id)
  if (!found || found.slot.params.type === kind) return
  if (!(await confirmLosingAutomation(id, found.slot.params.type, "Replace"))) {
    return
  }
  const wasSelected = effectsUi().selectedEffect === id
  const result = await dispatch({
    type: "replaceEffect",
    track: found.track.id,
    effect: id,
    kind,
  })
  if (!result) return
  const created = result.created[0]
  if (created === undefined) return
  // It takes the old one's place in the inspector too.
  const ui = effectsUi()
  ui.setCollapsed(created, ui.collapsed.includes(id))
  if (wasSelected) ui.selectEffect(created)
}

/** Puts every setting of an effect back to its default. */
export async function resetEffect(id: EffectId) {
  const found = findEffect(id)
  if (!found) return
  await dispatch({
    type: "setEffectParams",
    track: found.track.id,
    effect: id,
    params: effectDescriptor(found.slot.params.type).defaults,
  })
}

export function effectName(kind: EffectKind): string {
  return t(effectDescriptor(kind).name)
}

/** Whether an effect of this kind reports gain reduction. */
export function hasGainReduction(kind: EffectKind): boolean {
  return (
    kind === "compressor" ||
    kind === "limiter" ||
    // The multiband and mastering dynamics report through the same feed.
    kind === "multibandCompressor" ||
    kind === "multibandMaximizer" ||
    kind === "oneKnob" ||
    kind === "stageStack" ||
    kind === "masterLimiter"
  )
}
