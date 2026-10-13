import { create } from "zustand"

import { registry, type Action } from "@/lib/actions"
import { dynamicTitle } from "@/lib/i18n"

export const useSpectrumPanel = create(() => ({ open: false }))

export function closeSpectrum() {
  useSpectrumPanel.setState({ open: false })
}

const SPECTRUM_ACTIONS: Action[] = [
  {
    id: "view.spectrum",
    ...dynamicTitle("Show spectrum", {}),
    section: "View",
    keywords: "frequency power bins",
    run: () => useSpectrumPanel.setState({ open: true }),
  },
]

export function registerSpectrumActions(): () => void {
  const unregister = registry.register(SPECTRUM_ACTIONS)
  return () => {
    unregister()
    closeSpectrum()
  }
}
