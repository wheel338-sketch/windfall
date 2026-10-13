import { useEffect, useRef, useState } from "react"
import type { PluginTarget } from "@/bindings"
import { localizeBackendText } from "@/lib/i18n"
import { backend, errorMessage } from "@/lib/ipc"
import type { PluginNoteCapabilities } from "@/lib/ipc/backend"
import { useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"

export function useNativeNoteCapabilities(target: PluginTarget | undefined) {
  const revision = useProjectStore((state) => state.revision)
  const channel = target?.type === "instrument" ? target.channel : undefined
  const previous = useRef<{ channel?: number; generation?: number; capabilities?: PluginNoteCapabilities }>({})
  const [result, setResult] = useState<{ channel?: number; generation?: number; capabilities?: PluginNoteCapabilities; changed?: boolean; error?: string }>({})
  useEffect(() => {
    let live = true
    let pending = false
    const generation = getProjectGeneration()
    if (channel === undefined) return
    const refresh = async () => {
      if (pending) return
      pending = true
      try {
        const capabilities = await backend.pluginNoteCapabilities({ type: "instrument", channel })
        if (live && generation === getProjectGeneration()) {
          const before = previous.current
          const changed = before.channel === channel && before.generation === generation && before.capabilities !== undefined &&
            (before.capabilities.instances !== capabilities.instances || JSON.stringify(before.capabilities.nativePan) !== JSON.stringify(capabilities.nativePan) || JSON.stringify(before.capabilities.nativeVolume) !== JSON.stringify(capabilities.nativeVolume))
          previous.current = { channel, generation, capabilities }
          setResult((result) => ({ channel, generation, capabilities, changed: changed || (result.channel === channel && result.generation === generation && result.changed) }))
        }
      } catch (error) {
        if (live && generation === getProjectGeneration()) setResult({ channel, generation, error: localizeBackendText(errorMessage(error)) })
      } finally { pending = false }
    }
    void refresh()
    const timer = setInterval(() => void refresh(), 500)
    return () => { live = false; clearInterval(timer) }
  }, [channel, revision])
  return result.channel === channel && result.generation === getProjectGeneration() ? result : {}
}
export function panLawSupported(capabilities: PluginNoteCapabilities | undefined, midiChannel = 0) {
  return Number.isInteger(midiChannel) && midiChannel >= 0 && midiChannel < 16 && capabilities?.projectPanLaw === true && expressionSupported(capabilities, midiChannel, true, true)
}
export function expressionSupported(capabilities: PluginNoteCapabilities | undefined, midiChannel: number, pan: boolean, volume: boolean) {
  return Number.isInteger(midiChannel) && midiChannel >= 0 && midiChannel < 16 && capabilities?.instances === true && (!pan || capabilities.nativePan[midiChannel] === true) && (!volume || capabilities.nativeVolume[midiChannel] === true)
}
export function useNoteChannels(target: PluginTarget | undefined, capabilities: PluginNoteCapabilities | undefined) {
  const patterns = useProjectStore((state) => state.project.patterns)
  const channel = target?.type === "instrument" ? target.channel : undefined
  const channels = new Set(capabilities?.observedChannels ?? [])
  for (const pattern of patterns) for (const lane of pattern.lanes) {
    if (lane.channel === channel) for (const note of lane.notes) channels.add(note.expression?.colorGroup ?? 0)
  }
  return channels.size ? [...channels].sort((a, b) => a - b) : [0]
}
