import { localizeBackendText } from "@/lib/i18n"
import { message as i18nMessage } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import { ControllerLinksPanel } from "@/features/controllers/links-panel"
import { Input } from "@/components/ui/input"
import { useEffect, useRef, useState } from "react"
import type {
  ControllerState,
  MidiHardwareSettings,
  MidiHardwareState,
  MidiPort,
} from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Field,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from "@/components/ui/field"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { backend, errorMessage } from "@/lib/ipc"
import { useProjectGeneration, useProjectStore } from "@/lib/store"
import { releaseHardwareKeys } from "@/features/piano-roll/chords/held-keys"

type Option = { value: string; label: string }
const OFF = "off"
export function portOptions(
  ports: MidiPort[],
  chosen: string | null
): Option[] {
  const options = [
    { value: OFF, label: t("Disabled") },
    ...ports.map((port) => ({ value: `port:${port.id}`, label: port.name })),
  ]
  if (chosen !== null && !ports.some((port) => port.id === chosen)) {
    options.push({ value: `port:${chosen}`, label: t("Saved device unavailable") })
  }
  return options
}

function Choice({
  label,
  value,
  options,
  disabled,
  change,
}: {
  label: string
  value: string
  options: Option[]
  disabled: boolean
  change(value: string): void
}) {
  useLanguageStore((state) => state.language)
  return (
    <Field orientation="horizontal">
      <FieldLabel>{t(label)}</FieldLabel>
      <Select
        items={options.map((item) => ({ ...item, label: t(item.label) }))}
        value={value}
        disabled={disabled}
        onValueChange={(next) => {
          if (next !== null) change(next)
        }}
      >
        <SelectTrigger aria-label={t(label)} className="w-56">
          <SelectValue />
        </SelectTrigger>
        <SelectContent alignItemWithTrigger={false}>
          <SelectGroup>
            {options.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {t(option.label)}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </Field>
  )
}

const channelOptions = (): Option[] => Array.from({ length: 16 }, (_, index) => ({
  value: String(index + 1),
  label: i18nMessage("Channel {value0}", { value0: index + 1 }),
}))

const SWITCH: Option[] = [
  { value: "off", label: "Off" },
  { value: "on", label: "On" },
]
const NO_PROFILE = "none"
/** Grids recorded notes can be snapped to, in ticks of 960 a quarter note. */
const QUANTIZE: Option[] = [
  { value: "0", label: "Off" },
  { value: "120", label: "1/32 note" },
  { value: "240", label: "1/16 note" },
  { value: "480", label: "1/8 note" },
  { value: "960", label: "1/4 note" },
]

export function MidiHardwareSettingsSection() {
  useLanguageStore((state) => state.language)
  const [state, setState] = useState<MidiHardwareState | null>(null)
  const [controllers, setControllers] = useState<ControllerState | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const channels = useProjectStore((project) => project.project.channels)
  const projectGeneration = useProjectGeneration()
  const epoch = useRef(0)
  const working = useRef(false)
  const operation = useRef(0)

  useEffect(() => {
    if (state && state.connectedInputs.length === 0) releaseHardwareKeys()
  }, [state])

  useEffect(() => {
    const ticket = ++epoch.current
    let loading = false
    const load = async () => {
      if (loading || working.current) return
      loading = true
      const request = operation.current
      try {
        const result = await backend.midiHardwareState()
        const linked = await backend.controllerState()
        if (
          ticket === epoch.current &&
          request === operation.current &&
          !working.current
        ) {
          setState(result)
          setControllers(linked)
        }
      } catch (failure) {
        if (ticket === epoch.current && request === operation.current)
          setError(localizeBackendText(errorMessage(failure)))
      } finally {
        loading = false
      }
    }
    void load()
    const timer = window.setInterval(() => {
      void load()
    }, 500)
    return () => {
      epoch.current = ticket + 1
      window.clearInterval(timer)
    }
  }, [projectGeneration])

  async function run(work: () => Promise<MidiHardwareState>) {
    if (working.current) return
    const ticket = epoch.current
    operation.current++
    working.current = true
    setBusy(true)
    setError(null)
    try {
      const result = await work()
      if (ticket === epoch.current) {
        if (result.target !== state?.target || result.generation !== state?.generation ||
          state?.connectedInputs.some((input) => !result.connectedInputs.includes(input))) releaseHardwareKeys()
        setState(result)
      }
    } catch (failure) {
      if (ticket === epoch.current) setError(localizeBackendText(errorMessage(failure)))
    } finally {
      working.current = false
      setBusy(false)
    }
  }

  function configure(change: Partial<MidiHardwareSettings>) {
    if (state)
      void run(async () => {
        const result = await backend.midiHardwareConfigure({ ...state.settings, ...change })
        releaseHardwareKeys()
        return result
      })
  }
  async function configureControllers(change: Partial<ControllerState["settings"]>) {
    if (!controllers || working.current) return
    const ticket = epoch.current
    operation.current++
    working.current = true
    setBusy(true)
    setError(null)
    try {
      const next = await backend.controllerSettingsSet({ ...controllers.settings, ...change })
      if (ticket === epoch.current) setControllers(next)
    } catch (failure) { if (ticket === epoch.current) setError(localizeBackendText(errorMessage(failure))) }
    finally { working.current = false; setBusy(false) }
  }
  const disabled = busy || !state || backend.kind !== "tauri"
  return (
    <FieldSet>
      <FieldLegend>{t("MIDI hardware")}</FieldLegend>
      <FieldGroup>
        <Choice
          label={t("MIDI input")}
          value={state?.settings.input ? `port:${state.settings.input}` : OFF}
          options={portOptions(
            state?.inputs ?? [],
            state?.settings.input ?? null
          )}
          disabled={disabled}
          change={(value) =>
            configure({ input: value === OFF ? null : value.slice(5) })
          }
        />
        <Choice
          label={t("Input channel")}
          value={String(state?.settings.inputChannel ?? 0)}
          options={[{ value: "0", label: t("All channels") }, ...channelOptions()]}
          disabled={disabled}
          change={(value) =>
            configure({ inputChannel: value === "0" ? null : Number(value) })
          }
        />
        <Choice
          label={t("Audition destination")}
          value={String(state?.target ?? 0)}
          options={[
            { value: "0", label: t("No destination") },
            ...channels.map((channel) => ({
              value: String(channel.id),
              label: channel.name,
            })),
          ]}
          disabled={disabled}
          change={(value) => {
            if (state)
              void run(() =>
                backend.midiHardwareTarget(
                  value === "0" ? null : Number(value),
                  state.generation,
                  useProjectStore.getState().revision
                )
              )
          }}
        />
        <Choice
          label={t("Live MIDI output")}
          value={state?.settings.output ? `port:${state.settings.output}` : OFF}
          options={portOptions(
            state?.outputs ?? [],
            state?.settings.output ?? null
          )}
          disabled={disabled}
          change={(value) =>
            configure({ output: value === OFF ? null : value.slice(5) })
          }
        />
        {(state?.settings.extraOutputs ?? []).map((id, index) => <Choice key={`${index}:${id}`} label={i18nMessage("MIDI output slot {value0}", { value0: index + 1 })} value={id ? `port:${id}` : OFF} options={portOptions(state?.outputs ?? [], id || null).filter((option) => option.value === `port:${id}` || option.value === OFF || ![state?.settings.output, ...(state?.settings.extraOutputs ?? [])].includes(option.value.slice(5)))} disabled={disabled} change={(value) => configure({ extraOutputs: (state?.settings.extraOutputs ?? []).map((item, at) => at === index ? value === OFF ? "" : value.slice(5) : item) })} />)}
        <Button variant="outline" size="sm" disabled={disabled || (state?.settings.extraOutputs?.filter(Boolean).length ?? 0) >= 7 || !(state?.outputs ?? []).some((port) => port.id !== state?.settings.output && !(state?.settings.extraOutputs ?? []).includes(port.id))} onClick={() => {
          const next = state?.outputs.find((port) => port.id !== state.settings.output && !(state.settings.extraOutputs ?? []).includes(port.id))
          if (state && next) {
            const slots = [...(state.settings.extraOutputs ?? [])]
            const free = slots.indexOf("")
            if (free < 0) slots.push(next.id); else slots[free] = next.id
            configure({ extraOutputs: slots })
          }
        }}>{t("Add MIDI output")}</Button>
        <Choice
          label={t("Output channel")}
          value={String(state?.settings.outputChannel ?? 1)}
          options={channelOptions()}
          disabled={disabled}
          change={(value) => configure({ outputChannel: Number(value) })}
        />
        <Choice
          label={t("Controller profile")}
          value={state?.settings.inputProfile ?? NO_PROFILE}
          options={[
            { value: NO_PROFILE, label: t("None") },
            ...(controllers?.profiles ?? []).map((profile) => ({
              value: profile.id,
              label: profile.name,
            })),
          ]}
          disabled={disabled}
          change={(value) =>
            configure({ inputProfile: value === NO_PROFILE ? null : value })
          }
        />
        {(state?.settings.extraInputs ?? []).map((input, index) => <FieldSet key={input.id}>
          <FieldLegend>{t("Additional input")}{index + 1}</FieldLegend>
          <Choice label={i18nMessage("Additional input {value0}", { value0: index + 1 })} value={`port:${input.id}`} options={portOptions(state?.inputs ?? [], input.id).filter((option) => option.value === `port:${input.id}` || option.value === OFF || ![state?.settings.input, ...(state?.settings.extraInputs ?? []).map((item) => item.id)].includes(option.value.slice(5)))} disabled={disabled} change={(value) => configure({ extraInputs: (state?.settings.extraInputs ?? []).flatMap((item, at) => at === index ? value === OFF ? [] : [{ ...item, id: value.slice(5) }] : [item]) })} />
          <Choice label={i18nMessage("Additional input {value0} profile", { value0: index + 1 })} value={input.profile ?? NO_PROFILE} options={[{ value: NO_PROFILE, label: t("None") }, ...(controllers?.profiles ?? []).map((profile) => ({ value: profile.id, label: profile.name }))]} disabled={disabled} change={(value) => configure({ extraInputs: (state?.settings.extraInputs ?? []).map((item, at) => at === index ? { ...item, profile: value === NO_PROFILE ? null : value } : item) })} />
          <Button variant="outline" size="sm" disabled={disabled} onClick={() => configure({ extraInputs: (state?.settings.extraInputs ?? []).filter((_, at) => at !== index) })}>{i18nMessage("Remove additional input {index}", { index: index + 1 })}</Button>
        </FieldSet>)}
        <Button variant="outline" size="sm" disabled={disabled || (state?.settings.extraInputs.length ?? 0) >= 7 || !(state?.inputs ?? []).some((port) => port.id !== state?.settings.input && !(state?.settings.extraInputs ?? []).some((input) => input.id === port.id))} onClick={() => {
          const next = state?.inputs.find((port) => port.id !== state.settings.input && !state.settings.extraInputs.some((input) => input.id === port.id))
          if (state && next) configure({ extraInputs: [...state.settings.extraInputs, { id: next.id, profile: null }] })
        }}>{t("Add MIDI input")}</Button>
        <Field><FieldLabel htmlFor="midi-record-offset">{t("Recording offset (ms)")}</FieldLabel>
          <Input id="midi-record-offset" type="number" min={-500} max={500} step={1} defaultValue={controllers?.settings.recordOffsetMs ?? 0} key={controllers?.settings.recordOffsetMs ?? 0} disabled={disabled || !controllers} onBlur={(event) => {
            const value = Number(event.target.value)
            if (event.target.value.trim() && Number.isFinite(value) && Math.abs(value) <= 500) void configureControllers({ recordOffsetMs: value })
            else setError(t("Recording offset must be within -500 to 500 ms."))
          }} />
        </Field>
        <Choice
          label={t("Send MIDI clock")}
          value={state?.settings.sendClock ? "on" : "off"}
          options={SWITCH}
          disabled={disabled}
          change={(value) => configure({ sendClock: value === "on" })}
        />
        <Choice
          label={t("Follow MIDI clock")}
          value={state?.settings.followClock ? "on" : "off"}
          options={SWITCH}
          disabled={disabled}
          change={(value) => configure({ followClock: value === "on" })}
        />
        <Choice
          label={t("Controller pickup")}
          value={controllers?.settings.pickup ? "on" : "off"}
          options={SWITCH}
          disabled={disabled || !controllers}
          change={(value) => configureControllers({ pickup: value === "on" })}
        />
        <Choice
          label={t("Snap recorded notes")}
          value={String(controllers?.settings.recordQuantizeTicks ?? 0)}
          options={QUANTIZE}
          disabled={disabled || !controllers}
          change={(value) =>
            configureControllers({ recordQuantizeTicks: Number(value) })
          }
        />
      </FieldGroup>
      <div className="flex flex-wrap gap-2">
        <Button
          variant="outline"
          size="sm"
          disabled={disabled}
          onClick={() => {
            void run(() => backend.midiHardwareRefresh())
          }}
        >{t("Refresh ports")}</Button>
        <Button
          variant="outline"
          size="sm"
          disabled={disabled}
          onClick={() => configure({})}
        >{t("Reconnect devices")}</Button>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => {
            void backend
              .midiHardwarePanic()
              .then(releaseHardwareKeys)
              .catch((failure: unknown) => setError(localizeBackendText(errorMessage(failure))))
          }}
        >{t("Panic MIDI")}</Button>
      </div>
      <p aria-live="polite" className="text-muted-foreground">
        {state
          ? i18nMessage("Input {value0}; output {value1}. {value2} dropped events.", { value0: state.inputConnected ? t("connected") : t("disabled or unavailable"), value1: state.outputConnected ? t("connected") : t("disabled or unavailable"), value2: state.droppedEvents })
          : t("Loading MIDI devices…")}
      </p>
      {(error ?? state?.error) && (
        <Alert
          variant={
            backend.kind === "mock" && !error ? "default" : "destructive"
          }
        >
          <AlertDescription>{error ?? state?.error}</AlertDescription>
        </Alert>
      )}
      <ControllerLinksPanel />
      <p className="text-muted-foreground">{t("Play notes and sustain into the chosen channel. The destination resets when its channel is removed or another project opens. Output forwards live notes, and a MIDI output channel in the rack sends its pattern. Right-click a knob or fader to link it to a hardware control. A profile maps a known surface to the mixer and the transport.")}</p>
    </FieldSet>
  )
}
