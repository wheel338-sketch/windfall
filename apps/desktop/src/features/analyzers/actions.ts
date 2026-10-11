import { registry, type Action } from "@/lib/actions"

import { closeScope, openScope, setScopeSource } from "./scope-store"

const ANALYZER_ACTIONS: Action[] = [
  {
    id: "view.signalScope",
    title: "Show signal scope",
    section: "View",
    keywords:
      "oscilloscope vectorscope spectrum spectrogram peak meter analyzer level waveform phase",
    run: () => openScope(),
  },
  {
    id: "analyzers.sourceMix",
    title: "Signal scope: listen to the mix output",
    section: "View",
    keywords: "analyzer source master output",
    run: () => {
      setScopeSource({ type: "mixOutput" })
      openScope()
    },
  },
]

export function registerAnalyzerActions(): () => void {
  const unregister = registry.register(ANALYZER_ACTIONS)
  return () => {
    unregister()
    closeScope()
  }
}
