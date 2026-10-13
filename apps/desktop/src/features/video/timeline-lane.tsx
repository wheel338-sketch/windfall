import type { VideoPlacement } from "@/bindings"
import { Button } from "@/components/ui/button"
import { useMediaWindows } from "@/features/performance-media/store"
import { HEADER_WIDTH, SCROLLBAR_SIZE } from "@/features/playlist/layout"
import { useViewportValue, type GridMetrics } from "@/features/playlist/metrics"
import { useProjectStore } from "@/lib/store/project"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const NO_VIDEOS: VideoPlacement[] = []

/** Saved video placements share the playlist's actual horizontal viewport.
 * Opening a clip edits its checked start/duration/mute/source offset. */
export function VideoTimelineLane({ metrics }: { metrics: GridMetrics }) {
  const videos = useProjectStore((state) => state.project.settings.stageMedia?.videos ?? NO_VIDEOS)
  const scrollTick = useViewportValue(metrics, (viewport) => viewport.scrollTick)
  const pxPerTick = useViewportValue(metrics, (viewport) => viewport.pxPerTick)
  if (videos.length === 0) return null
  return <div className="flex h-9 shrink-0 border-b" aria-label={t("Timeline video lane")}>
    <div className="flex shrink-0 items-center px-2 text-xs text-muted-foreground" style={{ width: HEADER_WIDTH }}>{t("Video")}</div>
    <div className="relative min-w-0 flex-1 overflow-hidden bg-muted/20">
      {videos.map((video, index) => <Button key={`${index}:${video.path}`} variant="outline" size="sm" aria-label={message("Edit timeline video {index} at tick {startTick}", { index: plainNumber(index + 1), startTick: plainNumber(video.startTick) })} title={`${video.path} · ${video.startTick}–${video.startTick + video.lengthTicks}`} className="absolute top-1 truncate" style={{ left: (video.startTick - scrollTick) * pxPerTick, width: Math.max(12, video.lengthTicks * pxPerTick) }} onClick={() => useMediaWindows.setState({ video: true })}>{video.path.split(/[\\/]/).pop()}</Button>)}
    </div><div style={{ width: SCROLLBAR_SIZE }} />
  </div>
}
