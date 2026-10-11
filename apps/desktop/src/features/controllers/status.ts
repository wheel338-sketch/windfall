import { useEffect, useState } from "react"
import type { ControllerState } from "@/bindings"
import { backend } from "@/lib/ipc"
import { useProjectGeneration } from "@/lib/store/replaced"

/** A guarded status read; project replacement invalidates outstanding responses. */
export function useControllerStatus() {
  const generation = useProjectGeneration()
  const [state, setState] = useState<ControllerState | null>(null)
  useEffect(() => {
    let disposed = false
    let pending = false
    async function read() {
      if (pending) return
      pending = true
      try {
        const state = await backend.controllerState()
        if (!disposed) setState(state)
      } finally { pending = false }
    }
    void read().catch(() => {})
    const timer = window.setInterval(() => void read().catch(() => {}), 250)
    return () => { disposed = true; window.clearInterval(timer) }
  }, [generation])
  return { state, setState }
}
