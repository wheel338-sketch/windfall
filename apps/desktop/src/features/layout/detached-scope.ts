import { useEffect } from "react"
import { useUiStore, type PanelId } from "@/lib/store/ui"

/** Detached windows have no center tabs to provide the keyboard fallback.
 * Initialize their transient scope without changing the saved center tab. */
export function useDetachedScope(panel: PanelId) {
  useEffect(() => {
    useUiStore.getState().setActiveScope(panel)
    return () => {
      if (useUiStore.getState().activeScope === panel)
        useUiStore.getState().setActiveScope(null)
    }
  }, [panel])
}
