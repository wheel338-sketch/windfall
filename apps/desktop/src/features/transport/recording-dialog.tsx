import { useEffect } from "react"
import { ActionButton } from "@/components/action-button"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { useProjectStore } from "@/lib/store/project"
import { setTransport, useTransportStore } from "@/lib/store/transport"
import { nextCountInScale } from "./count-in-scale"
import { nextDialogBufferScale } from "./dialog-buffer-scale"
import { nextDialogLoopScale } from "./dialog-loop-scale"
import { nextDialogMonitorScale } from "./dialog-monitor-scale"
import { nextDialogOffsetScale } from "./dialog-offset-scale"
import { nextDialogStartScale } from "./dialog-start-scale"
import { refreshRecording, useRecordingStore } from "./recording-store"
import { translatedText, message, plainNumber } from "@/lib/i18n"
function Choice({
  label,
  value,
  items,
  onChange,
  disabled,
}: {
  label: string
  value: number
  items: { value: number; label: string }[]
  onChange: (value: number) => void
  disabled: boolean
}) {
  return (
    <Select
      items={items}
      value={value}
      disabled={disabled}
      onValueChange={(v: number | null) => {
        if (v !== null) onChange(v)
      }}
    >
      <SelectTrigger aria-label={label}>
        <SelectValue placeholder={translatedText("Not available")} />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {items.map((i) => (
            <SelectItem key={i.value} value={i.value}>
              {i.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}
export function RecordingDialog() {
  const countInBars = useTransportStore((state) => state.countInBars ?? 0)
  const countInRemaining = useTransportStore((state) => state.countInRemaining ?? 0)
  const s = useRecordingStore()
  const tracks = useProjectStore((p) => p.project.playlist.tracks)
  const mixerTracks = useProjectStore((p) => p.project.mixer.tracks)
  useEffect(() => {
    const timer = setInterval(() => {
      void refreshRecording()
    }, 500)
    return () => clearInterval(timer)
  }, [])
  const disabled = s.busy || s.state.active
  const input = s.inputs[s.device]
  const channels = Array.from({ length: input?.channels ?? 0 }, (_, value) => ({
    value,
    label: message("Input {value}", { value: plainNumber(value + 1) }),
  }))
  return (
    <Dialog
      open={s.open}
      onOpenChange={(open) => useRecordingStore.setState({ open })}
    >
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{translatedText("Record audio input")}</DialogTitle>
          <DialogDescription>
            {translatedText("Record a mono or stereo input into the playlist. Synchronize playback to the chosen start, use a count-in, and set an offset for your setup.")}
          </DialogDescription>
        </DialogHeader>
        <FieldGroup>
          <Field orientation="horizontal">
            <Checkbox id="record-armed" checked={s.armedTracks} disabled={disabled} onCheckedChange={(checked) => useRecordingStore.setState({ armedTracks: checked === true })} />
            <FieldLabel htmlFor="record-armed">{translatedText("Record all armed mixer tracks")}</FieldLabel>
          </Field>
          {s.armedTracks && <FieldDescription>{translatedText("Choose dry inputs or processed track sound and arm tracks in the mixer inspector. Each armed source gets its own aligned recording.")}{" "}{mixerTracks.filter((track) => track.recording?.armed).length}{" "}{translatedText("tracks armed.")}</FieldDescription>}
          <Field>
            <FieldLabel>{translatedText("Count-in")}</FieldLabel>
            <Choice label={translatedText("Count-in bars")} value={countInBars} disabled={disabled} items={[{ value: 0, label: translatedText("None") }, { value: 1, label: translatedText("1 bar") }, { value: 2, label: translatedText("2 bars") }, { value: 4, label: translatedText("4 bars") }, { value: 8, label: translatedText("8 bars") }]} onChange={(countInBars) => void setTransport({ countInBars })} />
            <div className="flex gap-2">
              {(["half", "double"] as const).map((factor) => (
                <Button
                  key={factor}
                  variant="outline"
                  size="sm"
                  aria-label={factor === "half" ? translatedText("Halve count-in") : translatedText("Double count-in")}
                  disabled={disabled || nextCountInScale(countInBars, factor) === null}
                  onClick={() => {
                    const recording = useRecordingStore.getState()
                    if (recording.busy || recording.state.active) return
                    const latest = useTransportStore.getState().countInBars ?? 0
                    const next = nextCountInScale(latest, factor)
                    if (next !== null) void setTransport({ countInBars: next })
                  }}
                >
                  {factor === "half" ? translatedText("Half") : translatedText("Double")}
                </Button>
              ))}
            </div>
            <FieldDescription>{translatedText("Count-in follows the song meter and tempo at the chosen start. Playback begins there after the clicks.")}</FieldDescription>
          </Field>
          <Field orientation="horizontal">
            <Checkbox id="record-sync" checked={s.synchronize || s.loopRecording || s.armedTracks || countInBars > 0} disabled={disabled || countInBars > 0 || s.loopRecording || s.armedTracks} onCheckedChange={(checked) => useRecordingStore.setState({ synchronize: checked === true })} />
            <FieldLabel htmlFor="record-sync">{translatedText("Start playback with recording")}</FieldLabel>
          </Field>
          <Field>
            <FieldLabel>{translatedText("Input device")}</FieldLabel>
            <Choice
              label={translatedText("Input device")}
              value={s.device}
              disabled={disabled || s.armedTracks}
              items={s.inputs.map((d, value) => ({
                value,
                label: `${d.host}: ${d.device}`,
              }))}
              onChange={(device) =>
                useRecordingStore.setState({ device, left: 0, right: -1, inputRate: 0 })
              }
            />
            <FieldDescription>
              {translatedText("Input opens before playback. Mono is stored as duplicated stereo.")}
            </FieldDescription>
          </Field>
          <Field>
            <FieldLabel>{translatedText("Input sample rate")}</FieldLabel>
            <Choice label={translatedText("Input sample rate")} value={s.inputRate} disabled={disabled} items={[{ value: 0, label: translatedText("Automatic") }, ...(input?.sampleRates ?? []).map((value) => ({ value, label: `${value} Hz` }))]} onChange={(inputRate) => useRecordingStore.setState({ inputRate })} />
            <FieldDescription>{translatedText("Audio is converted to the output sample rate when needed.")}</FieldDescription>
          </Field>
          <Field orientation="horizontal">
            <Checkbox id="record-drift" checked={s.driftCorrection} disabled={disabled} onCheckedChange={(checked) => useRecordingStore.setState({ driftCorrection: checked === true })} />
            <FieldLabel htmlFor="record-drift">{translatedText("Correct independent device clock drift")}</FieldLabel>
          </Field>
          <Field>
            <FieldLabel htmlFor="record-offset">{translatedText("Recording offset (milliseconds)")}</FieldLabel>
            <Input id="record-offset" type="number" min={-1000} max={1000} step={0.1} value={s.offsetMs} disabled={disabled} onChange={(event) => useRecordingStore.setState({ offsetMs: Math.max(-1000, Math.min(1000, Number(event.target.value))) })} />
            <FieldDescription>{translatedText("Positive values advance captured audio. Driver timestamps supply automatic input/output latency alignment.")}</FieldDescription>
            <div className="flex gap-2">
              {(["half", "double"] as const).map((factor) => (
                <Button
                  key={factor}
                  variant="outline"
                  size="sm"
                  disabled={disabled || nextDialogOffsetScale(s.offsetMs, factor) === null}
                  onClick={() => {
                    const current = useRecordingStore.getState()
                    if (current.busy || current.state.active) return
                    const next = nextDialogOffsetScale(current.offsetMs, factor)
                    if (next !== null) useRecordingStore.setState({ offsetMs: next })
                  }}
                >
                  {factor === "half" ? translatedText("Half") : translatedText("Double")}
                </Button>
              ))}
            </div>
          </Field>
          <Field>
            <FieldLabel>{translatedText("Left or mono channel")}</FieldLabel>
            <Choice
              label={translatedText("Left or mono channel")}
              value={s.left}
              disabled={disabled || s.armedTracks}
              items={channels}
              onChange={(left) => useRecordingStore.setState({ left })}
            />
          </Field>
          <Field>
            <FieldLabel>{translatedText("Right channel")}</FieldLabel>
            <Choice
              label={translatedText("Right channel")}
              value={s.right}
              disabled={disabled || s.armedTracks}
              items={[
                { value: -1, label: translatedText("Mono (duplicate left)") },
                ...channels,
              ]}
              onChange={(right) => useRecordingStore.setState({ right })}
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="record-start">{translatedText("Playlist start tick")}</FieldLabel>
            <Input
              id="record-start"
              type="number"
              min={0}
              step={1}
              value={s.start}
              disabled={disabled}
              onChange={(e) =>
                useRecordingStore.setState({
                  start: Math.max(0, Math.floor(Number(e.target.value))),
                })
              }
            />
            <div className="flex gap-2">
              {(["half", "double"] as const).map((factor) => (
                <Button
                  key={factor}
                  variant="outline"
                  size="sm"
                  aria-label={factor === "half" ? translatedText("Halve playlist start") : translatedText("Double playlist start")}
                  disabled={disabled || nextDialogStartScale(s.start, factor) === null}
                  onClick={() => {
                    const latest = useRecordingStore.getState()
                    if (latest.busy || latest.state.active) return
                    const next = nextDialogStartScale(latest.start, factor)
                    if (next !== null) useRecordingStore.setState({ start: next })
                  }}
                >
                  {factor === "half" ? translatedText("Half") : translatedText("Double")}
                </Button>
              ))}
            </div>
          </Field>
          <Field>
            <FieldLabel>{translatedText("Playlist track")}</FieldLabel>
            <Choice
              label={translatedText("Playlist track")}
              value={s.track}
              disabled={disabled}
              items={[
                { value: -1, label: translatedText("New recording track") },
                ...tracks.map((t) => ({ value: t.id, label: t.name })),
              ]}
              onChange={(track) => useRecordingStore.setState({ track })}
            />
          </Field>
          <Field orientation="horizontal">
            <Checkbox id="record-loop" checked={s.loopRecording} disabled={disabled} onCheckedChange={(checked) => useRecordingStore.setState({ loopRecording: checked === true })} />
            <FieldLabel htmlFor="record-loop">{translatedText("Record loop takes")}</FieldLabel>
          </Field>
          {s.loopRecording && <>
            <Field>
              <FieldLabel htmlFor="record-loop-end">{translatedText("Loop end tick")}</FieldLabel>
              <Input id="record-loop-end" type="number" min={s.start + 1} step={1} value={s.loopEnd} disabled={disabled} onChange={(event) => useRecordingStore.setState({ loopEnd: Math.max(s.start + 1, Math.floor(Number(event.target.value))) })} />
              <div className="flex gap-2">
                {(["half", "double"] as const).map((factor) => (
                  <Button
                    key={factor}
                    variant="outline"
                    size="sm"
                    aria-label={factor === "half" ? translatedText("Halve recording loop") : translatedText("Double recording loop")}
                    disabled={disabled || nextDialogLoopScale(s.start, s.loopEnd, factor) === null}
                    onClick={() => {
                      const latest = useRecordingStore.getState()
                      if (latest.busy || latest.state.active || latest.loopRecording !== true) return
                      const next = nextDialogLoopScale(latest.start, latest.loopEnd, factor)
                      if (next !== null) useRecordingStore.setState({ loopEnd: next })
                    }}
                  >
                    {factor === "half" ? translatedText("Halve loop") : translatedText("Double loop")}
                  </Button>
                ))}
              </div>
              <FieldDescription>{translatedText("Each pass from the start tick to this end becomes a take. The final partial pass is retained too.")}</FieldDescription>
            </Field>
            <Field orientation="horizontal">
              <Checkbox id="record-latest" checked={s.keepLatest} disabled={s.busy} onCheckedChange={(checked) => useRecordingStore.setState({ keepLatest: checked === true })} />
              <FieldLabel htmlFor="record-latest">{translatedText("Keep only the latest take")}</FieldLabel>
            </Field>
          </>}
          <Field orientation="horizontal">
            <Checkbox id="record-monitor" checked={s.monitor && !s.armedTracks} disabled={disabled || s.armedTracks} onCheckedChange={(checked) => useRecordingStore.setState({ monitor: checked === true })} />
            <FieldLabel htmlFor="record-monitor">{translatedText("Monitor input through Windfall")}</FieldLabel>
          </Field>
          {s.monitor && !s.armedTracks && <>
            <Field>
              <FieldLabel>{translatedText("Monitor mixer track")}</FieldLabel>
              <Choice label={translatedText("Monitor mixer track")} value={s.monitorTrack} disabled={disabled} items={mixerTracks.map((track) => ({ value: track.id, label: track.name }))} onChange={(monitorTrack) => useRecordingStore.setState({ monitorTrack })} />
              <FieldDescription>{translatedText("Use headphones to keep the output from feeding back into the microphone. Monitor effects are heard while the recording keeps the original input.")}</FieldDescription>
            </Field>
            <Field>
              <FieldLabel htmlFor="record-monitor-gain">{translatedText("Monitor level (%)")}</FieldLabel>
              <Input id="record-monitor-gain" type="number" min={0} max={100} step={1} value={Math.round(s.monitorGain * 100)} disabled={disabled} onChange={(event) => useRecordingStore.setState({ monitorGain: Math.max(0, Math.min(1, Number(event.target.value) / 100)) })} />
              <div className="flex gap-2">
                {(["half", "double"] as const).map((factor) => (
                  <Button
                    key={factor}
                    variant="outline"
                    size="sm"
                    aria-label={factor === "half" ? translatedText("Halve dialog monitor level") : translatedText("Double dialog monitor level")}
                    disabled={disabled || nextDialogMonitorScale(s.monitorGain, factor) === null}
                    onClick={() => {
                      const latest = useRecordingStore.getState()
                      if (latest.busy || latest.state.active || latest.monitor !== true || latest.armedTracks === true) return
                      const next = nextDialogMonitorScale(latest.monitorGain, factor)
                      if (next !== null) useRecordingStore.setState({ monitorGain: next })
                    }}
                  >
                    {factor === "half" ? translatedText("Half") : translatedText("Double")}
                  </Button>
                ))}
              </div>
            </Field>
            <Field>
              <FieldLabel htmlFor="record-monitor-buffer">{translatedText("Monitor buffer (milliseconds)")}</FieldLabel>
              <Input id="record-monitor-buffer" type="number" min={5} max={100} step={1} value={s.monitorBufferMs} disabled={disabled} onChange={(event) => useRecordingStore.setState({ monitorBufferMs: Math.max(5, Math.min(100, Math.round(Number(event.target.value)))) })} />
              <div className="flex gap-2">
                {(["half", "double"] as const).map((factor) => (
                  <Button
                    key={factor}
                    variant="outline"
                    size="sm"
                    aria-label={factor === "half" ? translatedText("Halve dialog monitor buffer") : translatedText("Double dialog monitor buffer")}
                    disabled={disabled || nextDialogBufferScale(s.monitorBufferMs, factor) === null}
                    onClick={() => {
                      const latest = useRecordingStore.getState()
                      if (latest.busy || latest.state.active || latest.monitor !== true || latest.armedTracks === true) return
                      const next = nextDialogBufferScale(latest.monitorBufferMs, factor)
                      if (next !== null) useRecordingStore.setState({ monitorBufferMs: next })
                    }}
                  >
                    {factor === "half" ? translatedText("Half") : translatedText("Double")}
                  </Button>
                ))}
              </div>
              <FieldDescription>{translatedText("A larger buffer accommodates driver scheduling delays and adds monitoring latency.")}</FieldDescription>
            </Field>
          </>}
        </FieldGroup>
        {s.inputs.length === 0 && !s.armedTracks && (
          <p>
            {translatedText("No audio inputs found. Recording needs the native desktop app and an available input device.")}
          </p>
        )}
        <p role="status">
          {s.state.active
            ? countInRemaining > 0 ? message("Count-in: {countInRemaining} beats remaining.", { countInRemaining: plainNumber(countInRemaining) }) : message("Recording {frames} seconds at {sampleRate} Hz. Stop and keep, or discard.", { frames: (s.state.frames / Math.max(1, s.state.sampleRate)).toFixed(1), sampleRate: plainNumber(s.state.sampleRate) })
            : translatedText("Ready. Recording sources are kept in Windfall’s recordings folder and copied into the project on save.")}
        </p>
        {s.state.active && s.state.alignment && <p className="text-sm text-muted-foreground">
          {translatedText("Input")}{" "}{s.state.alignment.inputSampleRate}{" "}{translatedText("Hz · input latency")}{" "}{s.state.alignment.inputLatencyMs.toFixed(1)}{" "}{translatedText("ms · output latency")}{" "}{s.state.alignment.outputLatencyMs.toFixed(1)}{" "}{translatedText("ms · drift")}{" "}{s.state.alignment.driftPpm.toFixed(1)} ppm.
          {s.state.alignment.measuredTimestamps ? translatedText(" Driver timestamps available.") : translatedText(" Timestamp estimate in use.")}
        </p>}
        {s.state.active && s.state.takes && s.state.takes.length > 0 && <fieldset className="max-h-40 space-y-2 overflow-y-auto rounded border p-3">
          <legend className="px-1 text-sm">{translatedText("Takes to keep")}</legend>
          {s.state.takes.map((take, index, takes) => <Field key={take.index} orientation="horizontal">
            <Checkbox id={`record-take-${take.index}`} checked={s.keepLatest ? index === takes.length - 1 : !s.excludedTakes.includes(take.index)} disabled={s.busy || s.keepLatest} onCheckedChange={(checked) => useRecordingStore.setState({ excludedTakes: checked === true ? s.excludedTakes.filter((value) => value !== take.index) : [...s.excludedTakes, take.index] })} />
            <FieldLabel htmlFor={`record-take-${take.index}`}>{translatedText("Take")}{" "}{take.index + 1} · {(take.frames / Math.max(1, s.state.sampleRate)).toFixed(1)} s{take.complete ? "" : translatedText(" (recording)")}</FieldLabel>
          </Field>)}
          <p className="text-xs text-muted-foreground">{translatedText("Kept takes share the loop start. The latest is audible; earlier takes are muted for auditioning in the playlist.")}</p>
        </fieldset>}
        {s.state.active && s.state.monitor && <p className="text-sm text-muted-foreground">{translatedText("Monitor buffer")}{" "}{s.state.monitor.bufferedMs.toFixed(1)}{" "}{translatedText("ms · dropped")}{" "}{s.state.monitor.droppedFrames}{" "}{translatedText("frames · starved")}{" "}{s.state.monitor.starvedFrames}{" "}{translatedText("frames.")}</p>}
        {s.state.active && s.state.tracks && <ul aria-label={translatedText("Recording tracks")} className="max-h-40 space-y-1 overflow-y-auto text-xs">
          {s.state.tracks.map((track) => <li key={track.mixerTrack} className="rounded border p-2">
            <p className="font-medium">{track.name} · {(track.frames / Math.max(1, s.state.sampleRate)).toFixed(1)} s · {mixerTracks.find((item) => item.id === track.mixerTrack)?.recording?.mode === "postEffects" ? translatedText("after effects") : mixerTracks.find((item) => item.id === track.mixerTrack)?.recording?.mode === "postFader" ? translatedText("after fader") : translatedText("dry input")}</p>
            <p className="text-muted-foreground">{translatedText("Source")}{" "}{track.alignment.inputSampleRate}{" "}{translatedText("Hz · latency")}{" "}{track.alignment.inputLatencyMs.toFixed(1)}{" "}{translatedText("ms · drift")}{" "}{track.alignment.driftPpm.toFixed(1)} ppm{track.monitor ? message(" · monitor {bufferedMs} ms / {droppedFrames} dropped frames", { bufferedMs: track.monitor.bufferedMs.toFixed(1), droppedFrames: plainNumber(track.monitor.droppedFrames) }) : ""}</p>
          </li>)}
        </ul>}
        {(s.error || s.state.error) && (
          <p role="alert" className="text-destructive">
            {s.error || s.state.error}
          </p>
        )}
        <div className="flex gap-2">
          <ActionButton action="recording.start">{translatedText("Start recording")}</ActionButton>
          <ActionButton action="recording.stop" variant="secondary">
            {translatedText("Stop and keep")}
          </ActionButton>
          <ActionButton action="recording.cancel" variant="outline">
            {translatedText("Discard")}
          </ActionButton>
        </div>
      </DialogContent>
    </Dialog>
  )
}
