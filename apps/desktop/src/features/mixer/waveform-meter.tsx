import { useEffect, useRef } from "react"
import { create } from "zustand"
import type { MixerWaveform, TrackId } from "@/bindings"
import { backend, errorMessage } from "@/lib/ipc"
import { subscribeRealtime } from "@/lib/store/realtime"
import { useProjectStore } from "@/lib/store/project"
import { onProjectReplaced } from "@/lib/store/replaced"
import { useUiStore } from "@/lib/store/ui"
import { resetPeak, subscribePeak, CLIP_GAIN } from "./peaks"
import { translatedText as t } from "@/lib/i18n"

const interested = new Map<TrackId, number>()
let notifyInterest: (() => void) | null = null
export const useWaveformUi = create<{ error: string }>(() => ({ error: "" }))

function registerTrack(id: TrackId) {
  interested.set(id, (interested.get(id) ?? 0) + 1)
  notifyInterest?.()
  return () => {
    const count = (interested.get(id) ?? 1) - 1
    if (count > 0) interested.set(id, count)
    else interested.delete(id)
    notifyInterest?.()
  }
}

/** One coalescing native interest owner; canvas subscriptions never issue IPC. */
export function watchMixerWaveforms(): () => void {
  let version = 0, drained = 0, running = false, disposed = false
  const sync = () => {
    version += 1
    if (disposed || running) return
    running = true
    void (async () => {
      let done = -1
      while (!disposed && done !== version) {
        done = version; drained = done
        const snapshot = useProjectStore.getState()
        if (!snapshot.ready) continue
        const ui = useUiStore.getState()
        const ids = ui.panels.mixer && ui.mixerMeterMode === "waveform" ? snapshot.project.mixer.tracks.filter((track) => interested.has(track.id)).slice(0, 128).map((track) => track.id) : []
        try {
          const guard = await backend.timelineState()
          if (disposed || done !== version) continue
          await backend.mixerWaveformTracks(ids, guard.generation, snapshot.revision)
          if (!disposed && done === version) useWaveformUi.setState({ error: "" })
        } catch (error) {
          if (!disposed && done === version) useWaveformUi.setState({ error: errorMessage(error) })
        }
      }
    })().finally(() => { running = false; if (!disposed && drained !== version) sync() })
  }
  notifyInterest = sync
  const off = [useProjectStore.subscribe(sync), useUiStore.subscribe((state, before) => { if (state.panels.mixer !== before.panels.mixer || state.mixerMeterMode !== before.mixerMeterMode) sync() }), onProjectReplaced(sync)]
  sync()
  return () => { disposed = true; version += 1; if (notifyInterest === sync) notifyInterest = null; off.forEach((unsubscribe) => unsubscribe()) }
}

export function StripWaveform({ id, vertical, wide, active }: { id: TrackId; vertical: boolean; wide: boolean; active: boolean }) {
  const canvas = useRef<HTMLCanvasElement>(null)
  const button = useRef<HTMLButtonElement>(null)
  const theme = useUiStore((state) => state.theme)
  useEffect(() => subscribePeak(id, (peak) => button.current?.toggleAttribute("data-clipped", peak > CLIP_GAIN)), [id])
  useEffect(() => {
    const element = canvas.current
    if (!element) return
    const context = element.getContext("2d")
    if (!context) return
    let latest: MixerWaveform | undefined
    let stamp = ""
    let resized = true
    const draw = () => {
      const width = element.clientWidth, height = element.clientHeight
      if (!width || !height) return
      const next = latest ? `${latest.epoch}:${latest.serial}` : "empty"
      if (!resized && stamp === next) return
      stamp = next; resized = false
      const ratio = window.devicePixelRatio || 1
      element.width = Math.max(1, Math.round(width * ratio)); element.height = Math.max(1, Math.round(height * ratio))
      context.setTransform(ratio, 0, 0, ratio, 0, 0)
      context.clearRect(0, 0, width, height)
      const style = getComputedStyle(element)
      const color = style.getPropertyValue("--wf-meter-low").trim() || "#72cc88"
      const clipped = style.getPropertyValue("--wf-meter-high").trim() || "#ef645c"
      const time = vertical ? height : width, amplitude = vertical ? width : height
      const lane = amplitude / 2
      const points = latest?.points ?? []
      context.lineWidth = 1
      // Left/right each have their own center line, even in a flat layout.
      context.strokeStyle = "#80808055"
      for (let side = 0; side < 2; side += 1) {
        const center = lane * (side + 0.5)
        context.beginPath()
        if (vertical) { context.moveTo(center, 0); context.lineTo(center, time) }
        else { context.moveTo(0, center); context.lineTo(time, center) }
        context.stroke()
      }
      for (let index = 0; index < points.length; index += 1) {
        const point = points[index]
        // Short histories are right-aligned on the full 64-bucket time span.
        const at = (64 - points.length + index + 0.5) / 64 * time
        for (let side = 0; side < 2; side += 1) {
          const min = point[side * 2], max = point[side * 2 + 1]
          const center = lane * (side + 0.5), scale = Math.max(0.5, lane / 2 - 0.5)
          const low = center - Math.max(-1, Math.min(1, min)) * scale
          const high = center - Math.max(-1, Math.min(1, max)) * scale
          context.strokeStyle = Math.max(Math.abs(min), Math.abs(max)) > 1 ? clipped : color
          context.beginPath()
          if (vertical) { context.moveTo(low, at); context.lineTo(high, at) }
          else { context.moveTo(at, low); context.lineTo(at, high) }
          context.stroke()
        }
      }
    }
    const observer = new ResizeObserver(() => { resized = true; draw() })
    observer.observe(element)
    const unregister = active ? registerTrack(id) : () => {}
    const clear = () => {
      latest = undefined; stamp = ""
      element.dataset.serial = ""; element.dataset.epoch = ""
      element.dataset.leftPeak = "0"; element.dataset.rightPeak = "0"
      draw()
    }
    const offReplace = onProjectReplaced(clear)
    const stop = active ? subscribeRealtime((frame) => {
      latest = frame.waveforms?.find((item) => item.track === id)
      element.dataset.serial = latest ? String(latest.serial) : ""
      element.dataset.epoch = latest ? String(latest.epoch) : ""
      for (let side = 0; side < 2; side += 1) {
        element.dataset[side === 0 ? "leftPeak" : "rightPeak"] = String(Math.max(0, ...(latest?.points ?? []).map((point) => Math.max(Math.abs(point[side * 2]), Math.abs(point[side * 2 + 1])))))
      }
      draw()
    }) : () => {}
    draw()
    return () => { unregister(); stop(); offReplace(); observer.disconnect() }
  }, [id, vertical, active, theme])
  return <button ref={button} type="button" aria-label={t("Stereo waveform history. Click to clear the held clip indication")}
    title={backend.kind === "mock" ? t("Audio waveforms are available in the desktop app") : t("Stereo waveform · about 320 ms · click to clear clip indication")}
    onClick={() => resetPeak(id)} className={`${vertical ? `h-full ${wide ? "w-4" : "w-3"}` : "h-1.5 w-full"} overflow-hidden rounded-[2px] bg-display outline-none data-clipped:ring-1 data-clipped:ring-destructive focus-visible:ring-2 focus-visible:ring-ring`}>
    <canvas ref={canvas} data-slot="mixer-waveform" data-track={id} aria-hidden="true" className="block h-full w-full" />
  </button>
}
