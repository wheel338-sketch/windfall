import { create } from "zustand"
import { persist } from "zustand/middleware"

import type { ChannelId } from "@/bindings"
import { onProjectReplaced } from "@/lib/store/replaced"

import {
  DEFAULT_LOW_KEY,
  DEFAULT_OCTAVES,
  DEFAULT_PAD_BASE,
  clampLowKey,
  clampPadBase,
  isOctaveRange,
  stepPadBase,
  type OctaveRange,
} from "./geometry"

export type TouchMode = "keyboard" | "pads"
export type PadLayout = "notes" | "channels"

type TouchControllersState = {
  /** Whether the window is showing. Never persisted. */
  open: boolean
  mode: TouchMode
  /** The C the keyboard starts on. */
  lowKey: number
  octaves: OctaveRange
  padLayout: PadLayout
  /** The key of the first pad in the Notes layout. */
  padBase: number
  /** Which 16 channels the Channels layout shows. Belongs to the project. */
  channelBank: number
  /** The channel picked in this window, or null to follow the selection. */
  channelOverride: ChannelId | null

  show(mode: TouchMode): void
  close(): void
  setMode(mode: TouchMode): void
  shiftOctave(direction: -1 | 1): void
  setOctaves(octaves: OctaveRange): void
  setPadLayout(layout: PadLayout): void
  shiftPadBase(direction: -1 | 1): void
  setChannelBank(bank: number): void
  setChannelOverride(channel: ChannelId | null): void
}

/**
 * The touch keyboard and pads. How they are set up (mode, octave, range,
 * pad layout and bank) survives a restart. Whether the window is open and
 * what belongs to one project do not, and held notes are not state at all.
 */
export const useTouchControllersStore = create<TouchControllersState>()(
  persist(
    (set) => ({
      open: false,
      mode: "keyboard",
      lowKey: DEFAULT_LOW_KEY,
      octaves: DEFAULT_OCTAVES,
      padLayout: "notes",
      padBase: DEFAULT_PAD_BASE,
      channelBank: 0,
      channelOverride: null,

      show: (mode) => set({ open: true, mode }),
      close: () => set({ open: false }),
      setMode: (mode) => set({ mode }),
      shiftOctave: (direction) =>
        set((state) => ({
          lowKey: clampLowKey(state.lowKey + direction * 12, state.octaves),
        })),
      setOctaves: (octaves) =>
        set((state) => ({
          octaves,
          lowKey: clampLowKey(state.lowKey, octaves),
        })),
      setPadLayout: (padLayout) => set({ padLayout }),
      shiftPadBase: (direction) =>
        set((state) => ({ padBase: stepPadBase(state.padBase, direction) })),
      setChannelBank: (bank) =>
        set({ channelBank: Math.max(0, Math.floor(bank)) }),
      setChannelOverride: (channelOverride) => set({ channelOverride }),
    }),
    {
      name: "windfall.touchControllers",
      version: 1,
      partialize: (state) => ({
        mode: state.mode,
        lowKey: state.lowKey,
        octaves: state.octaves,
        padLayout: state.padLayout,
        padBase: state.padBase,
      }),
      // Storage can hold anything; only values the controls can show come back.
      merge: (persisted, current) => {
        const saved: Partial<Record<keyof TouchControllersState, unknown>> =
          persisted && typeof persisted === "object" ? persisted : {}
        const octaves = isOctaveRange(saved.octaves)
          ? saved.octaves
          : current.octaves
        return {
          ...current,
          mode: saved.mode === "pads" ? "pads" : "keyboard",
          octaves,
          lowKey: clampLowKey(
            typeof saved.lowKey === "number" ? saved.lowKey : current.lowKey,
            octaves
          ),
          padLayout: saved.padLayout === "channels" ? "channels" : "notes",
          padBase: clampPadBase(
            typeof saved.padBase === "number" ? saved.padBase : current.padBase
          ),
        }
      },
    }
  )
)

// The channel picked for the window and the bank of channels on the pads
// were ids and positions in the project before.
onProjectReplaced(() =>
  useTouchControllersStore.setState({ channelOverride: null, channelBank: 0 })
)
