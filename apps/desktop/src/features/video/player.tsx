import { useEffect, useRef, useState } from "react"
import type { VideoPlacement } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { backend } from "@/lib/ipc"
import { mediaDocumentGuard } from "@/features/performance-media/store"
import { songTempoMap } from "@/lib/automation/tempo-map"
import { useProjectStore } from "@/lib/store/project"
import { useProjectGeneration } from "@/lib/store/replaced"
import { useRealtime } from "@/lib/store/realtime"
import { VideoFollower, videoTarget } from "./follower"
import { readRawAvi, type RawAvi } from "./avi"

/** A local file is read through a checked, bounded native operation then
 * decoded by the raw AVI reader or HTML video. Blob URLs are revoked on replacement/unmount. */
export function VideoPlayer({ placement }: { placement: VideoPlacement }) {
  const video = useRef<HTMLVideoElement>(null)
  const canvas = useRef<HTMLCanvasElement>(null)
  const pixels = useRef<{ avi: RawAvi; image: ImageData; index: number } | null>(null)
  const readout = useRef<HTMLParagraphElement>(null)
  const follower = useRef(new VideoFollower())
  const [loaded, setLoaded] = useState<{ path: string; generation: number; url: string | null; error: string | null; avi: RawAvi | null } | null>(null)
  const [playFailure, setPlayFailure] = useState<{ path: string; error: string } | null>(null)
  const playPending = useRef(false)
  const generation = useProjectGeneration()
  const revision = useProjectStore((state) => state.revision)
  const current = loaded?.path === placement.path && loaded.generation === generation ? loaded : null
  const url = current?.url ?? null
  const error = current?.error ?? null
  const playingError = playFailure?.path === placement.path ? playFailure.error : null

  useEffect(() => {
    let cancelled = false
    let owned: string | null = null
    follower.current.reset(); playPending.current = false; pixels.current = null
    void (async () => {
      const guard = await mediaDocumentGuard(generation)
      if (cancelled) return null
      return backend.mediaVideoRead(placement.path, guard.generation, guard.revision)
    })().then((bytes) => {
      if (cancelled || bytes === null) return
      const avi = readRawAvi(bytes)
      if (avi) { setLoaded({ path: placement.path, generation, url: null, error: null, avi }); return }
      const extension = placement.path.split(".").pop()?.toLowerCase()
      const mime = extension === "avi" ? "video/x-msvideo" : extension === "webm" ? "video/webm" : extension === "ogv" ? "video/ogg" : extension === "mov" ? "video/quicktime" : "video/mp4"
      owned = URL.createObjectURL(new Blob([bytes], { type: mime }))
      setLoaded({ path: placement.path, generation, url: owned, error: null, avi: null })
    }).catch((reason: unknown) => { if (!cancelled) setLoaded({ path: placement.path, generation, url: null, error: String(reason), avi: null }) })
    return () => { cancelled = true; pixels.current = null; if (owned) URL.revokeObjectURL(owned) }
  }, [placement.path, placement.muted, generation, revision])

  useRealtime((frame) => {
    if (current?.avi && canvas.current) {
      const avi = current.avi
      const map = songTempoMap(useProjectStore.getState().project)
      const target = videoTarget(placement, frame.mediaClock, (tick) => map.secondsAt(tick))
      const context = canvas.current.getContext("2d")
      if (!context) { if (!error) setLoaded({ ...current, error: "The webview canvas picture decoder is unavailable" }); return }
      if (pixels.current?.avi !== avi) pixels.current = { avi, image: context.createImageData(avi.width, avi.height), index: -2 }
      const index = target === null ? -1 : avi.frameAt(target)
      if (pixels.current.index !== index) {
        if (index < 0) context.clearRect(0, 0, avi.width, avi.height)
        else { avi.drawFrame(index, pixels.current.image.data); context.putImageData(pixels.current.image, 0, 0) }
        pixels.current.index = index
      }
      if (readout.current) readout.current.textContent = index < 0 ? "Outside the video picture range" : `Frame ${index + 1} | source ${target?.toFixed(3)} s | audio presentation clock`
      return
    }
    const element = video.current
    if (!element) return
    const clock = frame.mediaClock
    const project = useProjectStore.getState().project
    const map = songTempoMap(project)
    const target = videoTarget(placement, clock, (tick) => map.secondsAt(tick))
    const result = follower.current.follow(element, clock, target)
    if (readout.current) {
      readout.current.textContent = `Drift ${(result.drift * 1000).toFixed(1)} ms · ${result.correction} · driver latency ${clock ? clock.deviceLatencyFrames === null ? "unknown" : `${clock.deviceLatencyFrames} frames` : "unavailable"}`
      readout.current.dataset.correction = result.correction
    }
    if (clock?.playing && ["seek", "rate", "locked"].includes(result.correction) && element.paused && !playPending.current && !playingError) {
      playPending.current = true
      void element.play().catch((reason: unknown) => setPlayFailure({ path: placement.path, error: `Video playback refused: ${String(reason)}` })).finally(() => { playPending.current = false })
    }
  })

  return (
    <div className="flex flex-col gap-2">
      {current?.avi ? <canvas ref={canvas} width={current.avi.width} height={current.avi.height} role="img" aria-label="Timeline video" className="w-full rounded-md bg-background" /> : <video ref={video} src={url ?? undefined} muted playsInline preload="auto" aria-label="Timeline video" className="w-full rounded-md bg-background" onError={() => setLoaded({ path: placement.path, generation, url, error: "The webview cannot decode this video. Choose a supported local file.", avi: null })}>
        <track kind="captions" />
      </video>}
      <p ref={readout} className="text-xs text-muted-foreground" aria-live="off">Waiting for the audio presentation clock</p>
      {(error || playingError) && <Alert variant="destructive"><AlertDescription>{error ?? playingError}</AlertDescription></Alert>}
    </div>
  )
}
