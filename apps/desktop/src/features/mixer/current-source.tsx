import { create } from "zustand"
import { useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { backend, errorSource } from "@/lib/ipc"
import { onProjectReplaced } from "@/lib/store/replaced"
import { Button } from "@/components/ui/button"
import { dispatch } from "@/lib/store/project"
import { translatedText as t, localizeBackendText } from "@/lib/i18n"

export const useCurrentSource = create<{ track: number | null; error: string }>(() => ({ track: null, error: "" }))

/** One serial, coalescing selection owner per app window, including hidden mixers. */
export function watchCurrentSource(): () => void {
  let last = 0, version = 0, drained = 0, running = false, disposed = false
  const sync = () => {
    version += 1
    if (running || disposed) return
    running = true
    void (async () => {
      let done = -1
      while (!disposed && done !== version) {
        done = version
        drained = done
        const snapshot = useProjectStore.getState()
        if (!snapshot.ready) continue
        const tracks = snapshot.project.mixer.tracks
        const selected = tracks.find((track) => track.id === useUiStore.getState().selectedTrack)
        if (selected && !selected.current) last = selected.id
        if (!tracks.some((track) => track.id === last && !track.current)) last = 0
        const target = tracks.some((track) => track.current) ? last : null
        try {
          const guard = await backend.timelineState()
          if (disposed || done !== version) continue
          await backend.currentMixerTarget(target, guard.generation, snapshot.revision)
          if (!disposed && done === version) useCurrentSource.setState({ track: target, error: "" })
        } catch (error) {
          if (!disposed && done === version) useCurrentSource.setState({ track: null, error: errorSource(error) })
        }
      }
    })().finally(() => { running = false; if (!disposed && drained !== version) sync() })
  }
  const off = [useUiStore.subscribe((state, before) => { if (state.selectedTrack !== before.selectedTrack) sync() }), useProjectStore.subscribe(sync), onProjectReplaced(() => { last = 0; sync() })]
  sync()
  return () => { disposed = true; version += 1; off.forEach((unsubscribe) => unsubscribe()) }
}

export async function showCurrentUtility() {
  const result = await dispatch({ type: "ensureCurrentMixerTrack" })
  if (!result) return
  useUiStore.getState().setPanelVisible("mixer", true)
  useUiStore.getState().selectTrack(result.created[0])
}

export function CreateCurrentUtility() {
  return <Button variant="outline" size="sm" onClick={() => void showCurrentUtility()}>{t("Current")}</Button>
}

export function CurrentSourcePanel() {
  const source = useCurrentSource((state) => state.track)
  const error = useCurrentSource((state) => state.error)
  const name = useProjectStore((state) => state.project.mixer.tracks.find((track) => track.id === source)?.name)
  return <div className="shrink-0 border-b p-2 text-xs text-muted-foreground">
    <p>{t("Following:")}{" "}<span className="text-foreground">{name ?? t("No source")}</span></p>
    <p className="mt-1">{t("Select Master or an insert to change the source. Current keeps its own effects and meters; its processed copy stays outside the song output.")}</p>
    {backend.kind === "mock" && <p className="mt-1">{t("Native audio processing is available in the desktop app.")}</p>}
    {error && <p role="alert" className="mt-1 text-destructive">{localizeBackendText(error)}</p>}
  </div>
}
