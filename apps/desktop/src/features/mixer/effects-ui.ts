import { create } from "zustand"
import { persist } from "zustand/middleware"

import type { EffectId } from "@/bindings"
import { onProjectReplaced } from "@/lib/store/replaced"

/** Collapsed effects remembered at most. Old ones are forgotten first. */
const MAX_REMEMBERED = 256

type EffectsUiState = {
  /** Whether the selected track's effects are showing beside the strips. */
  inspectorOpen: boolean
  /**
   * Whether the track's own settings (sidechain, presets, EQ, input, output,
   * latency) are unfolded above its effects. Folded, the effects get the room.
   */
  trackSettingsOpen: boolean
  /** Effects whose editor is folded away in the inspector. */
  collapsed: EffectId[]
  /** The effect the effect actions act on, and whose slot is marked. */
  selectedEffect: EffectId | null
  /** The slot a key moved, which takes the focus back once it is in place. */
  focusing: EffectId | null
  /** The effect the inspector scrolls to next. */
  revealing: EffectId | null

  setInspectorOpen(open: boolean): void
  toggleInspector(): void
  setTrackSettingsOpen(open: boolean): void
  selectEffect(effect: EffectId | null): void
  setCollapsed(effect: EffectId, collapsed: boolean): void
}

/**
 * View state of the mixer's effects that is not part of the project.
 * Whether the inspector is open survives a restart, and its width is saved
 * with the other panel sizes. Which editors are folded does not: effects
 * are known by ids that start over in every project, so a fold kept for
 * later would land on whatever effect has that id next.
 */
export const useEffectsUi = create<EffectsUiState>()(
  persist(
    (set) => ({
      inspectorOpen: false,
      trackSettingsOpen: false,
      collapsed: [],
      selectedEffect: null,
      focusing: null,
      revealing: null,

      setInspectorOpen: (inspectorOpen) => set({ inspectorOpen }),
      toggleInspector: () =>
        set((state) => ({ inspectorOpen: !state.inspectorOpen })),
      setTrackSettingsOpen: (trackSettingsOpen) => set({ trackSettingsOpen }),
      selectEffect: (selectedEffect) => set({ selectedEffect }),
      setCollapsed: (effect, collapsed) =>
        set((state) => {
          const others = state.collapsed.filter((id) => id !== effect)
          return {
            collapsed: collapsed
              ? [...others, effect].slice(-MAX_REMEMBERED)
              : others,
          }
        }),
    }),
    {
      name: "windfall.mixer",
      version: 2,
      // Version 1 also kept the folded effects, by id.
      migrate: (saved) => ({
        inspectorOpen:
          (saved as { inspectorOpen?: unknown } | null)?.inspectorOpen === true,
      }),
      partialize: (state) => ({
        inspectorOpen: state.inspectorOpen,
        trackSettingsOpen: state.trackSettingsOpen,
      }),
    }
  )
)

onProjectReplaced(() =>
  useEffectsUi.setState({
    collapsed: [],
    selectedEffect: null,
    focusing: null,
    revealing: null,
  })
)
