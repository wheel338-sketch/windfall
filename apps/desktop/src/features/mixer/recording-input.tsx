import { useEffect, useState } from "react"
import type { MixerRecording, RecordingInput, TrackId } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { useProjectStore } from "@/lib/store/project"
import { backend, errorMessage } from "@/lib/ipc"
import { useRecordingStore } from "@/features/transport/recording-store"
import { patchTrack } from "./operations"
import { refreshInputMonitors, toggleInputMonitors, useInputMonitorStore } from "./input-monitor-store"
import { nextRecordingOffsetScale } from "./recording-offset-scale"
import { nextMonitorGainScale } from "./monitor-gain-scale"
import { nextMonitorBufferScale } from "./monitor-buffer-scale"
import { nextRecordingSource } from "./record-source-step"
import { translatedText as t, message, plainNumber, localizeBackendText } from "@/lib/i18n"

export const DEFAULT_RECORDING: MixerRecording = { input: null, armed: false, monitor: false, monitorGain: 0.5, monitorBufferMs: 20, offsetMs: 0, mode: "input" }
export function RecordArm({ track }: { track: TrackId }) {
  const recording = useProjectStore((state) => state.project.mixer.tracks.find((item) => item.id === track)?.recording)
  const active = useRecordingStore((state) => state.state.active || state.busy)
  const canArm = !!recording && (!!recording.input || (recording.mode ?? "input") !== "input")
  return <Button variant="ghost" size="icon-xs" aria-label={recording?.armed ? t("Disarm recording track") : t("Arm recording track")} aria-pressed={recording?.armed ?? false} disabled={active || !canArm} title={canArm ? t("Record arm") : t("Choose a recording source in the track inspector")} className={recording?.armed ? "text-destructive" : "text-muted-foreground"} onClick={(event) => {
    event.stopPropagation()
    if (recording) void patchTrack(track, { recording: { ...recording, armed: !recording.armed } })
  }}><span aria-hidden className="size-2 rounded-full bg-current" /></Button>
}
function Choice({ label, value, items, disabled, change }: { label: string, value: number, items: { value: number, label: string }[], disabled: boolean, change: (value: number) => void }) {
  return <Select items={items} value={value} disabled={disabled} onValueChange={(value: number | null) => { if (value !== null) change(value) }}>
    <SelectTrigger aria-label={label}><SelectValue /></SelectTrigger>
    <SelectContent><SelectGroup>{items.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}</SelectGroup></SelectContent>
  </Select>
}
export function MixerRecordingPanel({ track }: { track: TrackId }) {
  const recording = useProjectStore((state) => state.project.mixer.tracks.find((item) => item.id === track)?.recording) ?? DEFAULT_RECORDING
  const recordingActive = useRecordingStore((state) => state.state.active || state.busy)
  const monitors = useInputMonitorStore((state) => state.state)
  const monitorBusy = useInputMonitorStore((state) => state.busy)
  const monitorError = useInputMonitorStore((state) => state.error)
  const active = recordingActive || monitors.active || monitorBusy
  const heard = monitors.tracks.find((item) => item.mixerTrack === track)
  const [open, setOpen] = useState(false)
  const [inputs, setInputs] = useState<RecordingInput[]>([])
  const [error, setError] = useState("")
  useEffect(() => {
    if (!open) return
    void refreshInputMonitors()
    let current = true
    void backend.recordingInputs().then((inputs) => { if (current) { setInputs(inputs); setError("") } }).catch((error) => { if (current) setError(errorMessage(error)) })
    return () => { current = false }
  }, [open])
  const device = inputs.findIndex((input) => input.host === recording.input?.host && input.device === recording.input?.device)
  const unavailable = recording.input !== null && device < 0
  const channels = Array.from({ length: inputs[device]?.channels ?? Math.max(1, (recording.input?.left ?? 0) + 1, (recording.input?.right ?? 0) + 1) }, (_, value) => ({ value, label: message("Input {value}", { value: plainNumber(value + 1) }) }))
  const patch = (patch: Partial<MixerRecording>) => { void patchTrack(track, { recording: { ...recording, ...patch } }) }
  const stepRecordingSource = (direction: "previous" | "next") => {
    if (useRecordingStore.getState().state.active || useRecordingStore.getState().busy || useInputMonitorStore.getState().state.active || useInputMonitorStore.getState().busy) return
    const latest = useProjectStore.getState().project.mixer.tracks.find((item) => item.id === track)?.recording ?? DEFAULT_RECORDING
    const next = nextRecordingSource(latest.mode ?? "input", direction)
    if (next !== null) void patchTrack(track, { recording: { ...latest, mode: next, armed: next === "input" && !latest.input ? false : latest.armed } })
  }
  const scaleMonitorGain = (factor: "half" | "double") => {
    if (useRecordingStore.getState().state.active || useRecordingStore.getState().busy || useInputMonitorStore.getState().state.active || useInputMonitorStore.getState().busy) return
    const latest = useProjectStore.getState().project.mixer.tracks.find((item) => item.id === track)?.recording
    if (!latest || latest.monitor !== true) return
    const next = nextMonitorGainScale(latest.monitorGain, factor)
    if (next !== null) void patchTrack(track, { recording: { ...latest, monitorGain: next } })
  }
  const scaleRecordingOffset = (factor: "half" | "double") => {
    if (useRecordingStore.getState().state.active || useRecordingStore.getState().busy || useInputMonitorStore.getState().state.active || useInputMonitorStore.getState().busy) return
    const latest = useProjectStore.getState().project.mixer.tracks.find((item) => item.id === track)?.recording ?? DEFAULT_RECORDING
    const next = nextRecordingOffsetScale(latest.offsetMs, factor)
    if (next !== null) void patchTrack(track, { recording: { ...latest, offsetMs: next } })
  }
  const scaleMonitorBuffer = (factor: "half" | "double") => {
    if (useRecordingStore.getState().state.active || useRecordingStore.getState().busy || useInputMonitorStore.getState().state.active || useInputMonitorStore.getState().busy) return
    const latest = useProjectStore.getState().project.mixer.tracks.find((item) => item.id === track)?.recording
    if (!latest || latest.monitor !== true) return
    const next = nextMonitorBufferScale(latest.monitorBufferMs, factor)
    if (next !== null) void patchTrack(track, { recording: { ...latest, monitorBufferMs: next } })
  }
  return <div className="flex shrink-0 items-center gap-1 border-b p-1.5">
    <RecordArm track={track} />
    <Button variant={monitors.active ? "secondary" : "ghost"} size="sm" aria-pressed={monitors.active} aria-label={monitors.active ? t("Stop standalone input monitoring") : t("Start standalone input monitoring")} disabled={recordingActive || monitorBusy || (!monitors.active && !recording.monitor)} title={t("Listen to every enabled mixer input without recording")} onClick={() => void toggleInputMonitors()}>{monitors.active ? t("Stop inputs") : t("Listen")}</Button>
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger render={<Button variant="outline" size="sm" className="min-w-0 flex-1 truncate">{recording.input ? recording.input.right === null ? message("{device}: {left} mono", { device: recording.input.device, left: plainNumber(recording.input.left + 1) }) : message("{device}: {left} / {right}", { device: recording.input.device, left: plainNumber(recording.input.left + 1), right: plainNumber(recording.input.right + 1) }) : t("Input & recording")}</Button>} />
      <PopoverContent align="start" className="max-h-[80vh] w-80 overflow-y-auto">
        <FieldGroup>
          <Field><FieldLabel>{t("Recording source")}</FieldLabel><Choice label={t("Mixer recording source")} value={recording.mode === "postEffects" ? 1 : recording.mode === "postFader" ? 2 : 0} disabled={active} items={[{ value: 0, label: t("Dry hardware input") }, { value: 1, label: t("After track effects") }, { value: 2, label: t("After fader and pan") }]} change={(value) => patch({ mode: value === 1 ? "postEffects" : value === 2 ? "postFader" : "input", armed: value === 0 && !recording.input ? false : recording.armed })} />
            <div className="flex gap-2">
              <Button variant="outline" size="sm" aria-label={t("Choose the previous recording source")} disabled={active || nextRecordingSource(recording.mode ?? "input", "previous") === null} onClick={() => stepRecordingSource("previous")}>{t("Previous")}</Button>
              <Button variant="outline" size="sm" aria-label={t("Choose the next recording source")} disabled={active || nextRecordingSource(recording.mode ?? "input", "next") === null} onClick={() => stepRecordingSource("next")}>{t("Next")}</Button>
            </div>
            <FieldDescription>{(recording.mode ?? "input") === "input" ? t("Record the selected microphone or interface channels without track processing.") : t("Record this track’s notes, audio and incoming routes. Monitored hardware input is included. Printed clips use Direct output past mixer and Master processing.")}</FieldDescription></Field>
          <Field><FieldLabel>{t("External input")}</FieldLabel><Choice label={t("Track input device")} value={recording.input === null ? -1 : unavailable ? -2 : device} disabled={active} items={[{ value: -1, label: t("No external input") }, ...(unavailable ? [{ value: -2, label: message("Unavailable: {device}", { device: String(recording.input?.device) }) }] : []), ...inputs.map((input, value) => ({ value, label: `${input.host}: ${input.device}` }))]} change={(value) => {
            if (value === -1) { patch({ input: null, armed: (recording.mode ?? "input") === "input" ? false : recording.armed, monitor: false }); return }
            const input = inputs[value]
            if (input) patch({ input: { host: input.host, device: input.device, left: 0, right: null } })
          }} /><FieldDescription>{t("The saved device and channels feed this mixer track while its recording monitor is enabled.")}</FieldDescription></Field>
          {recording.input && <>
            <Field><FieldLabel>{t("Left / mono")}</FieldLabel><Choice label={t("Track left input")} value={recording.input.left} items={channels} disabled={active || unavailable} change={(left) => patch({ input: { ...recording.input!, left } })} /></Field>
            <Field><FieldLabel>{t("Right")}</FieldLabel><Choice label={t("Track right input")} value={recording.input.right ?? -1} items={[{ value: -1, label: t("Mono") }, ...channels]} disabled={active || unavailable} change={(right) => patch({ input: { ...recording.input!, right: right < 0 ? null : right } })} /></Field>
          </>}
          <Field orientation="horizontal"><Checkbox id={`track-arm-${track}`} checked={recording.armed} disabled={active || (!recording.input && (recording.mode ?? "input") === "input")} onCheckedChange={(armed) => patch({ armed: armed === true })} /><FieldLabel htmlFor={`track-arm-${track}`}>{t("Arm for multitrack recording")}</FieldLabel></Field>
          <Field orientation="horizontal"><Checkbox id={`track-monitor-${track}`} checked={recording.monitor} disabled={active || !recording.input} onCheckedChange={(monitor) => patch({ monitor: monitor === true })} /><FieldLabel htmlFor={`track-monitor-${track}`}>{t("Monitor hardware input")}</FieldLabel></Field>
          <FieldDescription>{t("Listen starts all enabled input routes independently of transport and recording. Starting a take closes standalone monitoring and uses the take's monitor settings.")}</FieldDescription>
          {recording.monitor && <>
            <Field><FieldLabel htmlFor={`track-monitor-gain-${track}`}>{t("Monitor level (%)")}</FieldLabel><Input id={`track-monitor-gain-${track}`} type="number" min={0} max={100} value={Math.round(recording.monitorGain * 100)} disabled={active} onChange={(event) => patch({ monitorGain: Math.max(0, Math.min(1, Number(event.target.value) / 100)) })} /><FieldDescription>{t("Use headphones to prevent microphone feedback. Effects are heard through this track; the selected recording source determines what is kept.")}</FieldDescription>
              <div className="flex gap-2">
                <Button variant="outline" size="sm" aria-label={t("Halve monitor level")} disabled={active || nextMonitorGainScale(recording.monitorGain, "half") === null} onClick={() => scaleMonitorGain("half")}>{t("Half")}</Button>
                <Button variant="outline" size="sm" aria-label={t("Double monitor level")} disabled={active || nextMonitorGainScale(recording.monitorGain, "double") === null} onClick={() => scaleMonitorGain("double")}>{t("Double")}</Button>
              </div>
            </Field>
            <Field><FieldLabel htmlFor={`track-monitor-buffer-${track}`}>{t("Monitor buffer (ms)")}</FieldLabel><Input id={`track-monitor-buffer-${track}`} type="number" min={5} max={100} value={recording.monitorBufferMs} disabled={active} onChange={(event) => patch({ monitorBufferMs: Math.max(5, Math.min(100, Math.round(Number(event.target.value)))) })} />
              <div className="flex gap-2">
                <Button variant="outline" size="sm" aria-label={t("Halve monitor buffer")} disabled={active || nextMonitorBufferScale(recording.monitorBufferMs, "half") === null} onClick={() => scaleMonitorBuffer("half")}>{t("Half")}</Button>
                <Button variant="outline" size="sm" aria-label={t("Double monitor buffer")} disabled={active || nextMonitorBufferScale(recording.monitorBufferMs, "double") === null} onClick={() => scaleMonitorBuffer("double")}>{t("Double")}</Button>
              </div>
            </Field>
          </>}
          <Field><FieldLabel htmlFor={`track-record-offset-${track}`}>{t("Track recording offset (ms)")}</FieldLabel><Input id={`track-record-offset-${track}`} type="number" min={-1000} max={1000} step={0.1} value={recording.offsetMs} disabled={active} onChange={(event) => patch({ offsetMs: Math.max(-1000, Math.min(1000, Number(event.target.value))) })} /><FieldDescription>{t("Added to the recording dialog offset. Positive values advance this track's captured audio.")}</FieldDescription>
            <div className="flex gap-2">
              <Button variant="outline" size="sm" disabled={active || nextRecordingOffsetScale(recording.offsetMs, "half") === null} onClick={() => scaleRecordingOffset("half")}>{t("Half")}</Button>
              <Button variant="outline" size="sm" disabled={active || nextRecordingOffsetScale(recording.offsetMs, "double") === null} onClick={() => scaleRecordingOffset("double")}>{t("Double")}</Button>
            </div>
          </Field>
        </FieldGroup>
        {heard && <div role="status" className="mt-2 text-xs text-muted-foreground">{heard.alignment.inputSampleRate}{" "}{t("Hz input ·")}{" "}{heard.monitor.bufferedMs.toFixed(1)}{" "}{t("ms buffered ·")}{" "}{heard.alignment.driftPpm.toFixed(1)}{" "}{t("ppm drift")}<br />{heard.monitor.droppedFrames}{" "}{t("dropped ·")}{" "}{heard.monitor.starvedFrames}{" "}{t("starved frames")}</div>}
        {monitorError && <p role="alert" className="mt-2 text-destructive">{localizeBackendText(monitorError)}</p>}
        {error && <p role="alert" className="mt-2 text-destructive">{error}</p>}
      </PopoverContent>
    </Popover>
  </div>
}
