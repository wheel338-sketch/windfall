import { create } from "zustand"

import { registry, type Action } from "@/lib/actions"
import { dynamicTitle } from "@/lib/i18n"

export const useSpectrogramPanel = create(() => ({ open: false }))

export function closeSpectrogram() {
  useSpectrogramPanel.setState({ open: false })
}

const SPECTROGRAM_ACTIONS: Action[] = [
  {
    id: "view.spectrogram",
    ...dynamicTitle("Show spectrogram", {}),
    section: "View",
    keywords: "frequency power history grid",
    run: () => useSpectrogramPanel.setState({ open: true }),
  },
]

export function registerSpectrogramActions(): () => void {
  const unregister = registry.register(SPECTROGRAM_ACTIONS)
  return () => {
    unregister()
    closeSpectrogram()
  }
}
