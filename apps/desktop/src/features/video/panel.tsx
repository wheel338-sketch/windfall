import { useState } from "react"
import type { VideoPlacement } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Switch } from "@/components/ui/switch"
import { MediaChoice, MediaNumber } from "@/features/performance-media/controls"
import { changeStageMedia, DEFAULT_STAGE_MEDIA, useMediaWindows } from "@/features/performance-media/store"
import { backend } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { VideoPlayer } from "./player"
import { message, plainNumber, translatedText as t } from "@/lib/i18n"

function PlacementEditor({ placement, index }: { placement: VideoPlacement; index: number }) {
  const tracks = useProjectStore((state) => state.project.mixer.tracks)
  const change = (patch: Partial<VideoPlacement>) => { void changeStageMedia((current) => ({ ...current, videos: current.videos.map((video, at) => at === index && video.path === placement.path ? { ...video, ...patch } : video) })) }
  return <section className="flex flex-col gap-3 rounded-md border p-3" aria-label={message("Video placement {index}", { index: plainNumber(index + 1) })}>
    <div className="flex items-center gap-2"><h3 className="min-w-0 flex-1 truncate text-sm">{placement.path.split(/[\\/]/).pop()}</h3><Button variant="outline" size="sm" onClick={() => { void changeStageMedia((current) => ({ ...current, videos: current.videos.filter((_, at) => at !== index) })) }}>{t("Remove video")}{" "}{index + 1}</Button></div>
    <FieldGroup className="grid grid-cols-3 gap-3"><MediaNumber label={message("Video {index} start tick", { index: plainNumber(index + 1) })} value={placement.startTick} max={2147483647} onCommit={(startTick) => change({ startTick })} /><MediaNumber label={message("Video {index} length ticks", { index: plainNumber(index + 1) })} value={placement.lengthTicks} min={1} max={2147483647} onCommit={(lengthTicks) => change({ lengthTicks })} /><MediaNumber label={message("Video {index} source offset seconds", { index: plainNumber(index + 1) })} value={placement.offsetSeconds} max={86400} step={0.01} onCommit={(offsetSeconds) => change({ offsetSeconds })} /><MediaChoice label={message("Video {index} mixer track", { index: plainNumber(index + 1) })} value={String(placement.mixerTrack)} options={tracks.map((track) => ({ value: String(track.id), label: track.name }))} onChange={(value) => change({ mixerTrack: Number(value) })} /><Field orientation="horizontal"><FieldLabel>{t("Mute video audio")}</FieldLabel><Switch aria-label={message("Mute video {index}", { index: plainNumber(index + 1) })} checked={placement.muted} onCheckedChange={(muted) => change({ muted })} /></Field></FieldGroup>
    <VideoPlayer placement={placement} />
  </section>
}

function VideoBody() {
  const media = useProjectStore((state) => state.project.settings.stageMedia) ?? DEFAULT_STAGE_MEDIA
  const [error, setError] = useState<string | null>(null)
  const add = async () => {
    setError(null)
    const generation = getProjectGeneration()
    try {
      const path = await backend.pickVideoFile()
      if (!path || generation !== getProjectGeneration()) return
      await changeStageMedia((current) => ({ ...current, videos: [...current.videos, { path, startTick: 0, lengthTicks: 3840, offsetSeconds: 0, muted: true, mixerTrack: 0 }] }))
    } catch (reason) { setError(String(reason)) }
  }
  return <div className="flex flex-col gap-3"><Button disabled={backend.kind !== "tauri" || media.videos.length >= 16} onClick={() => { void add() }}>{t("Place local video on timeline")}</Button><p className="text-xs text-muted-foreground">{t("Video placements follow song mode. Local files stay at their saved paths; raw RGB AVI pictures use the prepared frame reader; other pictures use the webview decoder. Audio uses the selected mixer track in playback and export. Native audio decoding supports AVI PCM and Ogg Vorbis; unsupported codecs report an error. Maximum file and decoded audio size: 64 MiB each, with a 128 MiB prepared video budget per project.")}</p>{error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}{media.videos.length === 0 ? <Empty><EmptyHeader><EmptyTitle>{t("No timeline video")}</EmptyTitle><EmptyDescription>{t("Choose a local video and set its start, duration and source offset.")}</EmptyDescription></EmptyHeader></Empty> : media.videos.map((placement, index) => <PlacementEditor key={`${index}:${placement.path}`} placement={placement} index={index} />)}</div>
}

export function VideoPanel() {
  const open = useMediaWindows((state) => state.video)
  return <Dialog open={open} onOpenChange={(video) => useMediaWindows.setState({ video })}><DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-3xl"><DialogHeader><DialogTitle>{t("Timeline video")}</DialogTitle><DialogDescription>{t("Place and preview video against the audio presentation clock, including seeks and loops.")}</DialogDescription></DialogHeader>{open && <VideoBody />}</DialogContent></Dialog>
}
