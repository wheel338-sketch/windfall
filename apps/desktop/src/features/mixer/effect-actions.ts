import { dynamicTitle } from "@/lib/i18n"
import type { EffectKind } from "@/bindings"
import { EFFECT_KINDS } from "@/features/params"
import type { Action, AppState } from "@/lib/actions"
import { ADD_EFFECT_SECTION, REPLACE_EFFECT_SECTION } from "@/lib/actions/menus"
import { useUiStore } from "@/lib/store"

import {
  addEffect,
  duplicateEffect,
  effectName,
  findEffect,
  MAX_EFFECT_SLOTS,
  moveEffectBy,
  openEffect,
  removeEffect,
  replaceEffect,
  resetEffect,
  toggleEffect,
  type FoundEffect,
} from "./effect-ops"
import { useEffectsUi } from "./effects-ui"

const SECTION = "Mixer"
const FULL = `Holds ${MAX_EFFECT_SLOTS} at most`

export const addEffectActionId = (kind: EffectKind) => `mixer.addEffect.${kind}`
export const replaceEffectActionId = (kind: EffectKind) =>
  `mixer.replaceEffect.${kind}`

function selectedTrack(state: AppState) {
  return state.document.project.mixer.tracks.find(
    (track) => track.id === state.ui.selectedTrack
  )
}

/** The selected effect. The mixer tells the registry when it changes. */
function target(state: AppState): FoundEffect | undefined {
  return findEffect(
    useEffectsUi.getState().selectedEffect,
    state.document.project.mixer.tracks
  )
}

function isFull(count: number | undefined): boolean {
  return (count ?? 0) >= MAX_EFFECT_SLOTS
}
function hosted(state: AppState, id: number): boolean {
  return (state.document.project.plugins ?? []).some(
    (binding) =>
      binding.target.type === "effect" && binding.target.effect === id
  )
}

/** Runs `work` on the selected effect. Does nothing when there is none. */
function onTarget(work: (id: number) => void | Promise<void>) {
  return () => {
    const id = useEffectsUi.getState().selectedEffect
    if (id !== null) return work(id)
  }
}

function showEffects() {
  const effects = useEffectsUi.getState()
  const ui = useUiStore.getState()
  // Closing them closes them wherever they are showing.
  if (ui.centerOverlay === "effects") {
    ui.setCenterOverlay(null)
    effects.setInspectorOpen(false)
    return
  }
  // Asking for the effects of a hidden mixer shows the mixer with them.
  if (!ui.panels.mixer) {
    ui.setPanelVisible("mixer", true)
    effects.setInspectorOpen(true)
    return
  }
  effects.toggleInspector()
}

/** Moves the effects into the editor area, or back beside the strips. */
function enlargeEffects() {
  const ui = useUiStore.getState()
  if (ui.centerOverlay === "effects") {
    ui.setCenterOverlay(null)
    return
  }
  // They come back to an open inspector, not to a closed one.
  useEffectsUi.getState().setInspectorOpen(true)
  ui.setCenterOverlay("effects")
}

/**
 * What can be done to the effects of the selected track and to the
 * selected effect. A slot's menu, the inspector, the Mixer menu and the
 * command palette all run these.
 */
export const EFFECT_ACTIONS: Action[] = [
  {
    id: "mixer.effects",
    title: "Show effects",
    section: SECTION,
    scope: "mixer",
    defaultShortcut: "E",
    keywords: "inspector chain inserts fx plugins editor",
    checked: (state) =>
      state.ui.centerOverlay === "effects" ||
      (state.ui.panels.mixer && useEffectsUi.getState().inspectorOpen),
    run: showEffects,
  },
  {
    id: "mixer.enlargeEffects",
    title: "Enlarge effects",
    section: SECTION,
    scope: "mixer",
    defaultShortcut: "Shift+E",
    keywords: "inspector editor maximize bigger detail fullscreen expand",
    checked: (state) => state.ui.centerOverlay === "effects",
    run: enlargeEffects,
  },
  {
    id: "mixer.effectsBack",
    title: "Return effects to the mixer",
    section: SECTION,
    scope: "effectInspector",
    defaultShortcut: "Escape",
    keywords: "shrink restore close enlarged",
    enabled: (state) => state.ui.centerOverlay === "effects",
    run: () => useUiStore.getState().setCenterOverlay(null),
  },
  ...EFFECT_KINDS.map((kind): Action => ({
    id: addEffectActionId(kind),
    ...dynamicTitle("Add {value0}", { value0: effectName(kind) }),
    section: ADD_EFFECT_SECTION,
    keywords: "effect insert fx plugin new",
    enabled: (state) => {
      const track = selectedTrack(state)
      return track !== undefined && !isFull(track.effects.length)
    },
    whyDisabled: (state) =>
      isFull(selectedTrack(state)?.effects.length) ? FULL : undefined,
    run: async () => {
      const track = useUiStore.getState().selectedTrack
      if (track !== null) await addEffect(track, kind)
    },
  })),
  {
    id: "mixer.openEffect",
    title: "Open effect",
    section: SECTION,
    keywords: "edit editor show",
    enabled: (state) => target(state) !== undefined,
    run: onTarget(openEffect),
  },
  {
    id: "mixer.bypassEffect",
    title: "Bypass effect",
    section: SECTION,
    keywords: "disable enable switch off on",
    enabled: (state) => target(state) !== undefined,
    checked: (state) => target(state)?.slot.enabled === false,
    run: onTarget(toggleEffect),
  },
  {
    id: "mixer.duplicateEffect",
    title: "Duplicate effect",
    section: SECTION,
    // The key is the effect's while its slot or its header has the focus.
    scope: "effect",
    editCommand: "duplicate",
    defaultShortcut: "Mod+D",
    keywords: "copy clone",
    enabled: (state) => {
      const found = target(state)
      return found !== undefined && !isFull(found.track.effects.length)
    },
    whyDisabled: (state) =>
      isFull(target(state)?.track.effects.length) ? FULL : undefined,
    run: onTarget(duplicateEffect),
  },
  {
    id: "mixer.moveEffectUp",
    title: "Move effect up",
    section: SECTION,
    scope: "mixer",
    defaultShortcut: "Alt+ArrowUp",
    repeats: true,
    keywords: "reorder earlier before",
    enabled: (state) => (target(state)?.index ?? 0) > 0,
    run: onTarget((id) => moveEffectBy(id, -1)),
  },
  {
    id: "mixer.moveEffectDown",
    title: "Move effect down",
    section: SECTION,
    scope: "mixer",
    defaultShortcut: "Alt+ArrowDown",
    repeats: true,
    keywords: "reorder later after",
    enabled: (state) => {
      const found = target(state)
      return found !== undefined && found.index < found.track.effects.length - 1
    },
    run: onTarget((id) => moveEffectBy(id, 1)),
  },
  ...EFFECT_KINDS.map((kind): Action => ({
    id: replaceEffectActionId(kind),
    ...dynamicTitle("Replace effect with {value0}", { value0: effectName(kind) }),
    section: REPLACE_EFFECT_SECTION,
    keywords: "swap change",
    enabled: (state) => {
      const found = target(state)
      return (
        found !== undefined &&
        (hosted(state, found.slot.id) || found.slot.params.type !== kind)
      )
    },
    run: onTarget((id) => replaceEffect(id, kind)),
  })),
  {
    id: "mixer.resetEffect",
    title: "Reset effect to defaults",
    section: SECTION,
    keywords: "initialize settings",
    enabled: (state) => {
      const found = target(state)
      return found !== undefined && !hosted(state, found.slot.id)
    },
    run: onTarget(resetEffect),
  },
  {
    id: "mixer.removeEffect",
    title: "Remove effect",
    section: SECTION,
    scope: "effect",
    editCommand: "delete",
    defaultShortcut: ["Delete", "Backspace"],
    keywords: "delete",
    enabled: (state) => target(state) !== undefined,
    run: onTarget(removeEffect),
  },
]
