import { useEffect, useState } from "react"
import type { PluginTarget } from "@/bindings"
import { localizeBackendText } from "@/lib/i18n"
import { backend, errorMessage } from "@/lib/ipc"
import type { PluginNoteCapabilities } from "@/lib/ipc/backend"
import { useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"

export function useNativeNoteCapabilities(target: PluginTarget | undefined) {
  const revision = useProjectStore((state) => state.revision)
  const channel = target?.type === "instrument" ? target.channel : undefined
  const [result, setResult] = useState<{ channel?: number; capabilities?: PluginNoteCapabilities; error?: string }>({})
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
        if (live && generation === getProjectGeneration()) setResult({ channel, capabilities })
      } catch (error) {
        if (live && generation === getProjectGeneration()) setResult({ channel, error: localizeBackendText(errorMessage(error)) })
      } finally { pending = false }
    }
    void refresh()
    const timer = setInterval(() => void refresh(), 500)
    return () => { live = false; clearInterval(timer) }
  }, [channel, revision])
  return result.channel === channel ? result : {}
}
export function panLawSupported(capabilities: PluginNoteCapabilities | undefined, midiChannel = 0) {
  return capabilities?.projectPanLaw === true && capabilities.instances === true && capabilities.nativePan[midiChannel] === true && capabilities.nativeVolume[midiChannel] === true
}
