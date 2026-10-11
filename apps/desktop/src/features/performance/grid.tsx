import { useEffect, useRef, useState } from "react"
import type { LaunchQuantization, PerformanceState } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { FieldGroup } from "@/components/ui/field"
import { learnTargets } from "@/features/controllers/menu"
import { MediaChoice } from "@/features/performance-media/controls"
import { changeStageMedia, DEFAULT_STAGE_MEDIA, useMediaWindows } from "@/features/performance-media/store"
import { reportError } from "@/lib/errors"
import { backend } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"

export function PerformanceGrid() {
  const project = useProjectStore((state) => state.project)
  const media = project.settings.stageMedia ?? DEFAULT_STAGE_MEDIA
  const [trackBank, setTrackBank] = useState("0")
  const [sceneBank, setSceneBank] = useState("0")
  const [status, setStatus] = useState<PerformanceState | null>(null)
  const tick = useRef<HTMLSpanElement>(null)
  useEffect(() => {
    let cancelled = false
    let pending = false
    let signature = ""
    let frame = 0
    let turns = 0
    const draw = () => {
      frame = requestAnimationFrame(draw)
      turns += 1
      if (pending || turns % 6 !== 0) return
      pending = true
      void backend.performanceState().then((next) => {
        if (cancelled) return
        if (tick.current) tick.current.textContent = `${next.tick.toFixed(1)} ticks`
        const key = JSON.stringify([next.available, next.droppedNotes, next.tracks])
        if (signature !== key) { signature = key; setStatus(next) }
      }).catch((error: unknown) => { if (!cancelled) reportError(error) }).finally(() => { pending = false })
    }
    draw()
    return () => { cancelled = true; cancelAnimationFrame(frame) }
  }, [])
  const send = async (track: number, scene: number | null) => {
    const state = useProjectStore.getState()
    try { await backend.performancePad(track, scene, getProjectGeneration(), state.revision) } catch (error) { reportError(error) }
  }
  const tracks = Array.from({ length: 8 }, (_, i) => Number(trackBank) * 8 + i)
  const scenes = Array.from({ length: 4 }, (_, i) => Number(sceneBank) * 4 + i)
  return <div className="flex flex-col gap-3">
    {status?.available === false && <Alert><AlertDescription>Launching needs the desktop audio engine. Pad assignments can still be saved here.</AlertDescription></Alert>}
    <FieldGroup className="grid grid-cols-3 gap-3">
      <MediaChoice<LaunchQuantization> label="Launch boundary" value={media.quantization} options={[{ value: "immediate", label: "Immediate" }, { value: "beat", label: "Beat" }, { value: "bar", label: "Bar" }]} onChange={(quantization) => { void changeStageMedia((current) => ({ ...current, quantization })) }} />
      <MediaChoice label="Track bank" value={trackBank} options={[0, 1, 2, 3].map((i) => ({ value: String(i), label: `${i * 8 + 1}–${i * 8 + 8}` }))} onChange={setTrackBank} />
      <MediaChoice label="Scene bank" value={sceneBank} options={[0, 1, 2, 3].map((i) => ({ value: String(i), label: `${i * 4 + 1}–${i * 4 + 4}` }))} onChange={setSceneBank} />
    </FieldGroup>
    <div className="overflow-x-auto"><table className="w-full" aria-label="Performance pads"><thead><tr><th>Scene</th>{tracks.map((track) => <th key={track}><div className="flex flex-col gap-1"><span>Track {track + 1}</span><Button variant="outline" size="sm" onClick={() => { void send(track, null) }}>Stop {track + 1}</Button></div></th>)}</tr></thead><tbody>{scenes.map((scene) => <tr key={scene}><th><Button variant="outline" size="sm" onClick={() => {
      const state = useProjectStore.getState()
      void backend.performanceScene(scene, getProjectGeneration(), state.revision).catch(reportError)
    }}>Scene {scene + 1}</Button></th>{tracks.map((track) => {
      const pad = media.pads.find((pad) => pad.track === track && pad.scene === scene)
      const state = status?.tracks[track]
      const name = project.patterns.find((pattern) => pattern.id === pad?.pattern)?.name ?? "Empty"
      return <td key={track} className="p-1"><div className="flex min-w-24 flex-col gap-1 rounded-md border p-1">
        <Button aria-label={`Launch track ${track + 1} scene ${scene + 1}`} variant={pad && state?.active === pad.pattern ? "default" : "outline"} disabled={!pad || status?.available === false} onClick={() => { void send(track, scene) }}>{name}</Button>
        {pad && state?.queued === pad.pattern && <Badge variant="secondary">Queued</Badge>}
        {state?.queuedStop && <Badge variant="secondary">Stop queued</Badge>}
        <MediaChoice label={`Assign track ${track + 1} scene ${scene + 1}`} value={pad ? String(pad.pattern) : "none"} options={[{ value: "none", label: "Empty" }, ...project.patterns.map((pattern) => ({ value: String(pattern.id), label: pattern.name }))]} onChange={(value) => { void changeStageMedia((current) => ({ ...current, pads: [...current.pads.filter((p) => p.track !== track || p.scene !== scene), ...(value === "none" ? [] : [{ track, scene, pattern: Number(value) }])] })) }} />
        <Button variant="ghost" size="sm" disabled={backend.kind !== "tauri"} onClick={() => { void learnTargets([{ type: "action", action: { performancePad: { track, scene } } }], false) }}>MIDI learn</Button>
      </div></td>
    })}</tr>)}</tbody></table></div>
    <div className="flex items-center gap-3"><Button onClick={() => { void backend.performanceStopAll().catch(reportError) }}>Stop all pads</Button><span ref={tick} className="text-xs text-muted-foreground" /><span className="text-xs text-muted-foreground">Dropped notes: {status?.droppedNotes ?? "0"}</span></div>
  </div>
}

export function PerformancePanel() {
  const open = useMediaWindows((state) => state.performance)
  return <Dialog open={open} onOpenChange={(performance) => useMediaWindows.setState({ performance })}><DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-6xl"><DialogHeader><DialogTitle>Performance grid</DialogTitle><DialogDescription>Assign patterns to pads, then launch or stop them on a musical boundary. Each track plays one pad. Keyboard and touch activate the same controls.</DialogDescription></DialogHeader>{open && <PerformanceGrid />}</DialogContent></Dialog>
}
