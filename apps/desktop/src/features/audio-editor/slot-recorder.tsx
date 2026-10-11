import { useEffect, useState } from "react"
import { Button } from "@/components/ui/button"
import { backend, errorMessage } from "@/lib/ipc"
import { receivePatch } from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { usePlaylistStore } from "@/features/playlist/store"
import type { AudioLoggerState } from "@/features/audio-logger/types"
import { AudioEditorButton } from "./index"

/** Uses the existing bounded native history worker. The callback copies only
 * this exact chain position, after the slot and before downstream effects. */
export function SlotRecorder({ effect }: { effect: number }) {
  const [state, setState] = useState<AudioLoggerState | null>(null)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [clip, setClip] = useState<number | null>(null)
  const [generation, setGeneration] = useState(getProjectGeneration)
  useEffect(() => {
    let cancelled = false
    const read = () => void backend.audioLoggerState().then(value => { if (!cancelled) setState(value) }).catch(reason => { if (!cancelled) setError(errorMessage(reason)) })
    read()
    const timer = setInterval(read, 500)
    const off = onProjectReplaced(() => { setClip(null); setGeneration(getProjectGeneration()); setPending(false) })
    return () => { cancelled = true; clearInterval(timer); off() }
  }, [])
  const source = state?.sources.find(s => s.source.type === "effect" && s.source.effect === effect)
  const recording = state?.settings.effects?.includes(effect) ?? false
  const othersRecording = (state?.settings.effects?.length ?? 0) > 0 && !recording
  async function start() {
    if (!state || pending || othersRecording) return
    const atGeneration = getProjectGeneration()
    setPending(true); setError(null)
    try {
      const next = await backend.audioLoggerConfigure({...state.settings,enabled:true,effects:[effect]})
      if (atGeneration === getProjectGeneration()) { setState(next); setGeneration(atGeneration) }
    } catch (reason) { if (atGeneration === getProjectGeneration()) setError(errorMessage(reason)) }
    finally { if (atGeneration === getProjectGeneration()) setPending(false) }
  }
  async function stop(keep: boolean) {
    if (!state || pending || generation !== getProjectGeneration()) return
    const atGeneration = generation
    setPending(true); setError(null)
    try {
      if (keep) {
        const result = await backend.audioLoggerDump({source:{type:"effect",effect},seconds:null,start:0,track:null})
        if (atGeneration !== getProjectGeneration()) return
        receivePatch(result.patch)
        const id = result.created.at(-1)
        if (id !== undefined) { usePlaylistStore.getState().select([id]); setClip(id) }
      }
      const next = await backend.audioLoggerConfigure({...state.settings,effects:(state.settings.effects ?? []).filter(id=>id!==effect)})
      if (atGeneration === getProjectGeneration()) setState(next)
    } catch (reason) { if (atGeneration === getProjectGeneration()) setError(errorMessage(reason)) }
    finally { if (atGeneration === getProjectGeneration()) setPending(false) }
  }
  return <section aria-label="Audio editor recorder" className="mt-3 flex flex-col gap-2 rounded-md border p-2">
    <p className="text-sm font-medium">Audio editor recorder</p>
    <p className="text-xs text-muted-foreground">Records what passes through this slot, before later effects and the fader. Insert a Send tap slot to place a transparent recorder between effects. History length follows the audio logger settings.</p>
    <output aria-label="Slot recorded duration">{(source?.availableSeconds ?? 0).toFixed(3)} s{source?.droppedFrames ? ` · ${source.droppedFrames} dropped frames` : ""}</output>
    {error && <p role="alert">{error}</p>}
    {state?.error && <p role="alert">{state.error}</p>}
    {source?.error && <p role="alert">{source.error}</p>}
    <div className="flex flex-wrap gap-2">
      {!recording ? <Button size="sm" variant="outline" disabled={pending || !state || othersRecording} onClick={() => void start()}>Record this slot</Button> : <>
        <Button size="sm" disabled={pending || !source?.availableFrames} onClick={() => void stop(true)}>Stop and edit slot recording</Button>
        <Button size="sm" variant="outline" disabled={pending} onClick={() => void stop(false)}>Discard slot recording</Button>
      </>}
      {clip !== null && <AudioEditorButton key={clip} clip={clip} defaultOpen />}
    </div>
  </section>
}
