import { useEffect, useRef, useState } from "react"
import type { VisualScene, VisualSettings } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { FieldGroup } from "@/components/ui/field"
import { useAnalyzerFrames, useAnalyzerSelection } from "@/features/analyzers/feed"
import { AnalyzerSourceSelect } from "@/features/analyzers/source-select"
import { AnalyzerStatusLine } from "@/features/analyzers/status"
import type { AnalyzerFrame, AnalyzerSource } from "@/features/analyzers/types"
import { MediaChoice, MediaNumber } from "@/features/performance-media/controls"
import { changeStageMedia, DEFAULT_STAGE_MEDIA, mediaDocumentGuard, useMediaWindows } from "@/features/performance-media/store"
import { backend } from "@/lib/ipc"
import { songTempoMap } from "@/lib/automation/tempo-map"
import { getProjectGeneration } from "@/lib/store/replaced"
import { useProjectStore } from "@/lib/store/project"
import { exportVisualAvi } from "./avi"
import { NoteCharacter } from "./character"
import { renderVisual, signalFromAnalyzer, visualSongFrame } from "./render"

const SLOT = "visualizer"
export function downloadMedia(bytes: Uint8Array, mime: string, name: string) {
  const owned = new Uint8Array(bytes.length)
  owned.set(bytes)
  const url = URL.createObjectURL(new Blob([owned], { type: mime }))
  const link = document.createElement("a")
  link.href = url; link.download = name; link.click()
  requestAnimationFrame(() => URL.revokeObjectURL(url))
}

export function VisualizerCanvas({ settings }: { settings: VisualSettings }) {
  const canvas = useRef<HTMLCanvasElement>(null)
  const latest = useRef<AnalyzerFrame | null>(null)
  const draw = (frame: AnalyzerFrame | null) => {
    latest.current = frame
    const element = canvas.current
    if (!element) return
    element.dataset.live = signalFromAnalyzer(frame) ? "true" : "false"
    const context = element.getContext("2d")
    if (!context) return
    const data = context.createImageData(320, 180)
    const phase = frame ? visualSongFrame(songTempoMap(useProjectStore.getState().project).secondsAt(frame.tick)) : 0
    data.data.set(renderVisual(320, 180, settings, signalFromAnalyzer(frame), phase))
    context.putImageData(data, 0, 0)
  }
  useAnalyzerFrames(SLOT, { paint: draw, onClear: () => draw(null) })
  useEffect(() => { draw(latest.current) })
  return <canvas ref={canvas} width={320} height={180} role="img" aria-label="Audio reactive scene" data-live="false" className="aspect-video w-full rounded-md" />
}

function VisualizerBody() {
  const project = useProjectStore((state) => state.project)
  const media = project.settings.stageMedia ?? DEFAULT_STAGE_MEDIA
  const [source, setSource] = useState<AnalyzerSource>({ type: "mixOutput" })
  const [start, setStart] = useState(0)
  const [end, setEnd] = useState(1920)
  const [exporting, setExporting] = useState(false)
  const [progress, setProgress] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const cancel = useRef(false)
  useEffect(() => () => { cancel.current = true }, [])
  useAnalyzerSelection(SLOT, { source, want: { spectrum: true, scope: false, vector: false, envelope: false } })
  const exportVideo = async () => {
    setError(null); setExporting(true); setProgress(0); cancel.current = false
    const state = useProjectStore.getState()
    const generation = getProjectGeneration()
    const visual = state.project.settings.stageMedia?.visual ?? DEFAULT_STAGE_MEDIA.visual
    try {
      const guard = await mediaDocumentGuard(generation, state.revision)
      const audio = await backend.mediaRenderAudio({ start, end }, guard.generation, guard.revision)
      if (cancel.current || generation !== getProjectGeneration()) throw new Error("Video export cancelled")
      const phase = visualSongFrame(songTempoMap(state.project).secondsAt(start))
      const result = await exportVisualAvi(audio.samples, audio.sampleRate, visual, 320, 180, () => cancel.current, setProgress, phase)
      if (cancel.current || generation !== getProjectGeneration()) throw new Error("Video export cancelled")
      downloadMedia(result.bytes, "video/x-msvideo", "windfall-visual.avi")
      downloadMedia(new TextEncoder().encode(JSON.stringify({ ...result.metadata, range: { start, end }, visual }, null, 2)), "application/json", "windfall-visual.json")
    } catch (reason) { if (!cancel.current) setError(String(reason)) } finally { setExporting(false) }
  }
  return <div className="flex flex-col gap-3">
    <div className="flex flex-wrap items-center gap-2"><AnalyzerSourceSelect source={source} onChange={setSource} /><AnalyzerStatusLine slot={SLOT} /></div>
    <VisualizerCanvas settings={media.visual} />
    <FieldGroup className="grid grid-cols-3 gap-3">
      <MediaChoice<VisualScene> label="Scene" value={media.visual.scene} options={[{ value: "orbit", label: "Orbit" }, { value: "bars", label: "Light bars" }, { value: "ribbon", label: "Ribbon" }]} onChange={(scene) => { void changeStageMedia((current) => ({ ...current, visual: { ...current.visual, scene } })) }} />
      <MediaNumber label="Intensity" value={media.visual.intensity} max={4} step={0.1} onCommit={(intensity) => { void changeStageMedia((current) => ({ ...current, visual: { ...current.visual, intensity } })) }} />
      <MediaNumber label="Hue" value={media.visual.hue} max={360} onCommit={(hue) => { void changeStageMedia((current) => ({ ...current, visual: { ...current.visual, hue } })) }} />
    </FieldGroup>
    <FieldGroup className="grid grid-cols-2 gap-3"><MediaNumber label="Export start tick" value={start} max={2147483647} onCommit={setStart} /><MediaNumber label="Export end tick" value={end} min={1} max={2147483647} onCommit={setEnd} /></FieldGroup>
    <p className="text-xs text-muted-foreground">AVI export includes the real offline mix as stereo PCM and raw video frames at 24 fps. Short ranges only: up to 30 seconds and 128 MiB. Compressed H.264/AAC codecs remain a user licensing decision.</p>
    <div className="flex items-center gap-3"><Button disabled={exporting || source.type !== "mixOutput" || backend.kind !== "tauri" || end <= start} onClick={() => { void exportVideo() }}>Export synchronized AVI</Button>{exporting && <><span role="status">{Math.round(progress * 100)}%</span><Button variant="outline" onClick={() => { cancel.current = true }}>Cancel export</Button></>}</div>
    {source.type !== "mixOutput" && <p className="text-xs text-muted-foreground">Select Mix output to export this scene with the mix.</p>}
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    <MediaChoice label="Moss note channel" value={media.danceChannel === null ? "none" : String(media.danceChannel)} options={[{ value: "none", label: "No channel" }, ...project.channels.map((channel) => ({ value: String(channel.id), label: channel.name }))]} onChange={(value) => { void changeStageMedia((current) => ({ ...current, danceChannel: value === "none" ? null : Number(value) })) }} />
    <NoteCharacter channel={media.danceChannel} />
  </div>
}

export function VisualizerPanel() {
  const open = useMediaWindows((state) => state.visualizer)
  return <Dialog open={open} onOpenChange={(visualizer) => useMediaWindows.setState({ visualizer })}><DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-3xl"><DialogHeader><DialogTitle>Visual stage</DialogTitle><DialogDescription>Windfall scenes follow the selected audio analyzer. Moss dances to actual held notes on the chosen channel.</DialogDescription></DialogHeader>{open && <VisualizerBody />}</DialogContent></Dialog>
}
