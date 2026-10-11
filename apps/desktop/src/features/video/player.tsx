import { useEffect, useRef, useState } from "react"
import type { VideoPlacement } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { backend } from "@/lib/ipc"
import { songTempoMap } from "@/lib/automation/tempo-map"
import { useProjectStore } from "@/lib/store/project"
import { useProjectGeneration } from "@/lib/store/replaced"
import { useRealtime } from "@/lib/store/realtime"
import { VideoFollower, videoTarget } from "./follower"

/** A local file is read through a checked, bounded native operation then
 * decoded by HTML video. Blob URLs are revoked on replacement/unmount. */
export function VideoPlayer({ placement }: { placement: VideoPlacement }) {
  const video = useRef<HTMLVideoElement>(null)
  const readout = useRef<HTMLParagraphElement>(null)
  const follower = useRef(new VideoFollower())
  const [loaded, setLoaded] = useState<{ path: string; generation: number; url: string | null; error: string | null } | null>(null)
  const [playFailure, setPlayFailure] = useState<{ path: string; error: string } | null>(null)
  const playPending = useRef(false)
  const generation = useProjectGeneration()
  const current = loaded?.path === placement.path && loaded.generation === generation ? loaded : null
  const url = current?.url ?? null
  const error = current?.error ?? null
  const playingError = playFailure?.path === placement.path ? playFailure.error : null

  useEffect(() => {
    let cancelled = false
    let owned: string | null = null
    follower.current.reset(); playPending.current = false
    void backend.mediaVideoRead(placement.path, generation).then((bytes) => {
      if (cancelled) return
      const extension = placement.path.split(".").pop()?.toLowerCase()
      const mime = extension === "webm" ? "video/webm" : extension === "ogv" ? "video/ogg" : extension === "mov" ? "video/quicktime" : "video/mp4"
      owned = URL.createObjectURL(new Blob([bytes], { type: mime }))
      setLoaded({ path: placement.path, generation, url: owned, error: null })
    }).catch((reason: unknown) => { if (!cancelled) setLoaded({ path: placement.path, generation, url: null, error: String(reason) }) })
    return () => { cancelled = true; if (owned) URL.revokeObjectURL(owned) }
  }, [placement.path, generation])

  useRealtime((frame) => {
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
      <video ref={video} src={url ?? undefined} muted={placement.muted} playsInline preload="auto" aria-label="Timeline video" className="w-full rounded-md bg-background" onError={() => setLoaded({ path: placement.path, generation, url, error: "The webview cannot decode this video. Choose a supported local file." })}>
        <track kind="captions" />
      </video>
      <p ref={readout} className="text-xs text-muted-foreground" aria-live="off">Waiting for the audio presentation clock</p>
      {(error || playingError) && <Alert variant="destructive"><AlertDescription>{error ?? playingError}</AlertDescription></Alert>}
    </div>
  )
}
