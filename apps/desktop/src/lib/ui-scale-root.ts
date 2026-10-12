import { useEffect, useLayoutEffect } from "react"

import { useUiStore } from "@/lib/store/ui"
import { applyUiScale } from "@/lib/ui-scale"
import { backend } from "@/lib/ipc"

/** One scale for the app, detached panel views, showcases and all portals. */
export function useApplicationScale() {
  const scale = useUiStore((state) => state.uiScale)
  // Already-open detached views share local preferences with the main view.
  // Rehydrate through persist, which does not echo a write to other windows.
  useEffect(() => {
    const changed = (event: StorageEvent) => {
      if (event.key === "windfall.ui") void useUiStore.persist.rehydrate()
    }
    window.addEventListener("storage", changed)
    return () => window.removeEventListener("storage", changed)
  }, [])
  useLayoutEffect(() => {
    const apply = () => applyUiScale(scale)
    apply()
    const stopMonitor = backend.onWindowScale?.(apply)
    window.addEventListener("resize", apply)
    return () => { stopMonitor?.(); window.removeEventListener("resize", apply) }
  }, [scale])
}
