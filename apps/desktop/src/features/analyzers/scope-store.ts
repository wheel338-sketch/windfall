import { create } from "zustand"
import { persist } from "zustand/middleware"

import { onProjectReplaced } from "@/lib/store/replaced"

import type { AnalyzerSource, AnalyzerWant } from "./types"
import { translatedText as t } from "@/lib/i18n"

export const SCOPE_VIEWS = [
  "meter",
  "oscilloscope",
  "spectrum",
  "spectrogram",
  "vectorscope",
] as const
export type ScopeView = (typeof SCOPE_VIEWS)[number]

export const SCOPE_VIEW_NAMES: Record<ScopeView, string> = {
  get meter() { return t("Peak meter") },
  get oscilloscope() { return t("Oscilloscope") },
  get spectrum() { return t("Spectrum") },
  get spectrogram() { return t("Spectrogram") },
  get vectorscope() { return t("Vectorscope") },
}

/** Samples the oscilloscope draws across its width. */
export const SCOPE_SPANS = [256, 512, 1024] as const
export type ScopeSpan = (typeof SCOPE_SPANS)[number]

/** The quietest level the spectrum and the meter show, in dB. */
export const SCOPE_FLOORS = [-60, -90, -120] as const
export type ScopeFloor = (typeof SCOPE_FLOORS)[number]

const MIX: AnalyzerSource = { type: "mixOutput" }

type ScopeState = {
  /** Session only. */
  open: boolean
  /** Session only: it names a track of the open project. */
  source: AnalyzerSource
  views: ScopeView[]
  span: ScopeSpan
  floorDb: ScopeFloor
  /** Whether the oscilloscope waits for an upward zero crossing. */
  triggered: boolean
}

export const useScopeStore = create<ScopeState>()(
  persist(
    (): ScopeState => ({
      open: false,
      source: MIX,
      views: ["meter", "oscilloscope", "spectrum"],
      span: 512,
      floorDb: -90,
      triggered: true,
    }),
    {
      name: "windfall.signal-scope",
      version: 1,
      partialize: (state) => ({
        views: state.views,
        span: state.span,
        floorDb: state.floorDb,
        triggered: state.triggered,
      }),
      merge: (persisted, current) => {
        const saved = (
          persisted && typeof persisted === "object" ? persisted : {}
        ) as Partial<ScopeState>
        const views = Array.isArray(saved.views)
          ? SCOPE_VIEWS.filter((view) => saved.views?.includes(view))
          : current.views
        return {
          ...current,
          views,
          span: SCOPE_SPANS.find((span) => span === saved.span) ?? current.span,
          floorDb:
            SCOPE_FLOORS.find((floor) => floor === saved.floorDb) ??
            current.floorDb,
          triggered:
            typeof saved.triggered === "boolean"
              ? saved.triggered
              : current.triggered,
        }
      },
    }
  )
)

export function openScope() {
  useScopeStore.setState({ open: true })
}

export function closeScope() {
  useScopeStore.setState({ open: false })
}

export function setScopeSource(source: AnalyzerSource) {
  useScopeStore.setState({ source })
}

/** Shows or hides one view. The views keep their fixed order. */
export function setScopeView(view: ScopeView, shown: boolean) {
  useScopeStore.setState((state) => ({
    views: SCOPE_VIEWS.filter((item) =>
      item === view ? shown : state.views.includes(item)
    ),
  }))
}

/** What the shown views need from the analyzer, and nothing more. */
export function scopeWant(views: readonly ScopeView[]): AnalyzerWant {
  return {
    scope: views.includes("oscilloscope"),
    spectrum: views.includes("spectrum") || views.includes("spectrogram"),
    vector: views.includes("vectorscope"),
    envelope: false,
  }
}

// A track of the old project means nothing in the new one, even where the
// ids happen to match.
onProjectReplaced(() => setScopeSource(MIX))
