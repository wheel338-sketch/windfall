import { useState } from "react"
import type { TrackId } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { useProjectStore, dispatch } from "@/lib/store/project"
import { useRecordingStore } from "@/features/transport/recording-store"
import { errorMessage } from "@/lib/ipc"
import { LATENCY_PRESETS, nextLatency } from "./latency-presets"
import { nextLatencyPreset } from "./latency-preset-step"
import { nextLatencyScale } from "./latency-scale"
import { translatedText as t } from "@/lib/i18n"

export function LatencyOffsetPanel({ track }: { track: TrackId }) {
  const value = useProjectStore((state) => state.project.mixer.tracks.find((item) => item.id === track)?.latencyOffsetMs ?? 0)
  const recording = useRecordingStore((state) => state.state.active || state.busy)
  const [draft, setDraft] = useState(String(value))
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const [previous, setPrevious] = useState({ track, value })
  if (previous.track !== track || previous.value !== value) {
    setPrevious({ track, value })
    setDraft(String(value))
    setError("")
  }
  const apply = async (offset: number, reset = false) => {
    if (busy || recording) return
    if (!Number.isFinite(offset) || Math.abs(offset) > 1000 || !reset && !draft.trim()) {
      setError(t("Enter a correction between -1000 and 1000 ms.")); return
    }
    setBusy(true); setError("")
    try {
      if (!await dispatch({ type: "updateMixerTrack", id: track, patch: { latencyOffsetMs: offset } })) setError(t("The correction was not applied."))
      else setDraft(String(offset))
    } catch (error) { setError(errorMessage(error)) }
    finally { setBusy(false) }
  }
  const applyPreset = async (preset: number) => {
    const recordingState = useRecordingStore.getState()
    if (busy || recordingState.state.active || recordingState.busy) return
    const latest = useProjectStore.getState().project.mixer.tracks.find((item) => item.id === track)?.latencyOffsetMs ?? 0
    if (nextLatency(latest, preset) === null) return
    setBusy(true); setError("")
    try {
      if (!await dispatch({ type: "updateMixerTrack", id: track, patch: { latencyOffsetMs: preset } })) setError(t("The correction was not applied."))
      else { setDraft(String(preset)); setError("") }
    } catch (error) { setError(errorMessage(error)) }
    finally { setBusy(false) }
  }
  const applyPresetStep = async (direction: "previous" | "next") => {
    const recordingState = useRecordingStore.getState()
    if (busy || recordingState.state.active || recordingState.busy) return
    const latest = useProjectStore.getState().project.mixer.tracks.find((item) => item.id === track)?.latencyOffsetMs ?? 0
    const next = nextLatencyPreset(latest, direction)
    if (next === null) return
    setBusy(true); setError("")
    try {
      if (!await dispatch({ type: "updateMixerTrack", id: track, patch: { latencyOffsetMs: next } })) setError(t("The correction was not applied."))
      else { setDraft(String(next)); setError("") }
    } catch (error) { setError(errorMessage(error)) }
    finally { setBusy(false) }
  }
  const applyScale = async (factor: "half" | "double") => {
    const recordingState = useRecordingStore.getState()
    if (busy || recordingState.state.active || recordingState.busy) return
    const latest = useProjectStore.getState().project.mixer.tracks.find((item) => item.id === track)?.latencyOffsetMs ?? 0
    const next = nextLatencyScale(latest, factor)
    if (next === null) return
    setBusy(true); setError("")
    try {
      if (!await dispatch({ type: "updateMixerTrack", id: track, patch: { latencyOffsetMs: next } })) setError(t("The correction was not applied."))
      else { setDraft(String(next)); setError("") }
    } catch (error) { setError(errorMessage(error)) }
    finally { setBusy(false) }
  }
  return <div className="shrink-0 border-b p-1.5"><Popover>
    <PopoverTrigger render={<Button variant="outline" size="sm" className="w-full justify-start truncate">{t("Latency correction:")}{" "}{value > 0 ? "+" : ""}{value} ms</Button>} />
    <PopoverContent align="start" className="w-80"><FieldGroup>
      <Field><FieldLabel htmlFor={`latency-offset-${track}`}>{t("Declared latency correction (ms)")}</FieldLabel>
        <Input id={`latency-offset-${track}`} type="number" min={-1000} max={1000} step={0.01} value={draft} disabled={recording || busy} onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void apply(Number(draft)); if (event.key === "Escape") { setDraft(String(value)); setError("") } }} />
        <FieldDescription>{t("Positive values account for delay that a plugin or external signal does not report. Negative values reduce an overstated report. The engine compensates other paths using the corrected declaration.")}</FieldDescription>
      </Field>
      <FieldDescription>{t("The declaration is rounded to device samples and cannot become negative. Use the recording offset control to move captured takes. This correction changes alignment on playback and export.")}</FieldDescription>
      <div className="flex flex-wrap gap-2"><Button size="sm" disabled={recording || busy} onClick={() => void apply(Number(draft))}>{t("Apply")}</Button><Button size="sm" variant="outline" disabled={recording || busy || value === 0} onClick={() => void apply(0, true)}>{t("Reset")}</Button>
        {LATENCY_PRESETS.map((preset) => <Button key={preset.label} size="sm" variant="outline" disabled={recording || busy || nextLatency(value, preset.value) === null} onClick={() => void applyPreset(preset.value)}>{preset.label}</Button>)}
        <Button size="sm" variant="outline" aria-label={t("Choose the previous latency preset")} disabled={recording || busy || nextLatencyPreset(value, "previous") === null} onClick={() => void applyPresetStep("previous")}>{t("Previous")}</Button>
        <Button size="sm" variant="outline" aria-label={t("Choose the next latency preset")} disabled={recording || busy || nextLatencyPreset(value, "next") === null} onClick={() => void applyPresetStep("next")}>{t("Next")}</Button>
        <Button size="sm" variant="outline" disabled={recording || busy || nextLatencyScale(value, "half") === null} onClick={() => void applyScale("half")}>{t("Half")}</Button>
        <Button size="sm" variant="outline" disabled={recording || busy || nextLatencyScale(value, "double") === null} onClick={() => void applyScale("double")}>{t("Double")}</Button>
      </div>
      {error && <p role="alert" className="text-destructive">{error}</p>}
    </FieldGroup></PopoverContent>
  </Popover></div>
}
