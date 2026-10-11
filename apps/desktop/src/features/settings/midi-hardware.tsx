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

type Option = { value: string; label: string }
const OFF = "off"
export function portOptions(
  ports: MidiPort[],
  chosen: string | null
): Option[] {
  const options = [
    { value: OFF, label: "Disabled" },
    ...ports.map((port) => ({ value: `port:${port.id}`, label: port.name })),
  ]
  if (chosen !== null && !ports.some((port) => port.id === chosen)) {
    options.push({ value: `port:${chosen}`, label: "Saved device unavailable" })
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
  return (
    <Field orientation="horizontal">
      <FieldLabel>{label}</FieldLabel>
      <Select
        items={options}
        value={value}
        disabled={disabled}
        onValueChange={(next) => {
          if (next !== null) change(next)
        }}
      >
        <SelectTrigger aria-label={label} className="w-56">
          <SelectValue />
        </SelectTrigger>
        <SelectContent alignItemWithTrigger={false}>
          <SelectGroup>
            {options.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </Field>
  )
}

const CHANNELS: Option[] = Array.from({ length: 16 }, (_, index) => ({
  value: String(index + 1),
  label: `Channel ${index + 1}`,
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
          setError(errorMessage(failure))
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
      if (ticket === epoch.current) setState(result)
    } catch (failure) {
      if (ticket === epoch.current) setError(errorMessage(failure))
    } finally {
      working.current = false
      setBusy(false)
    }
  }

  function configure(change: Partial<MidiHardwareSettings>) {
    if (state)
      void run(() =>
        backend.midiHardwareConfigure({ ...state.settings, ...change })
      )
  }
  function configureControllers(change: Partial<ControllerState["settings"]>) {
    if (!controllers || working.current) return
    setError(null)
    backend
      .controllerSettingsSet({ ...controllers.settings, ...change })
      .then(setControllers)
      .catch((failure: unknown) => setError(errorMessage(failure)))
  }
  const disabled = busy || !state || backend.kind !== "tauri"
  return (
    <FieldSet>
      <FieldLegend>MIDI hardware</FieldLegend>
      <FieldGroup>
        <Choice
          label="MIDI input"
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
          label="Input channel"
          value={String(state?.settings.inputChannel ?? 0)}
          options={[{ value: "0", label: "All channels" }, ...CHANNELS]}
          disabled={disabled}
          change={(value) =>
            configure({ inputChannel: value === "0" ? null : Number(value) })
          }
        />
        <Choice
          label="Audition destination"
          value={String(state?.target ?? 0)}
          options={[
            { value: "0", label: "No destination" },
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
          label="Live MIDI output"
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
        <Choice
          label="Output channel"
          value={String(state?.settings.outputChannel ?? 1)}
          options={CHANNELS}
          disabled={disabled}
          change={(value) => configure({ outputChannel: Number(value) })}
        />
        <Choice
          label="Controller profile"
          value={state?.settings.inputProfile ?? NO_PROFILE}
          options={[
            { value: NO_PROFILE, label: "None" },
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
        <Choice
          label="Send MIDI clock"
          value={state?.settings.sendClock ? "on" : "off"}
          options={SWITCH}
          disabled={disabled}
          change={(value) => configure({ sendClock: value === "on" })}
        />
        <Choice
          label="Follow MIDI clock"
          value={state?.settings.followClock ? "on" : "off"}
          options={SWITCH}
          disabled={disabled}
          change={(value) => configure({ followClock: value === "on" })}
        />
        <Choice
          label="Controller pickup"
          value={controllers?.settings.pickup ? "on" : "off"}
          options={SWITCH}
          disabled={disabled || !controllers}
          change={(value) => configureControllers({ pickup: value === "on" })}
        />
        <Choice
          label="Snap recorded notes"
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
        >
          Refresh ports
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={disabled}
          onClick={() => configure({})}
        >
          Reconnect devices
        </Button>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => {
            void backend
              .midiHardwarePanic()
              .catch((failure: unknown) => setError(errorMessage(failure)))
          }}
        >
          Panic MIDI
        </Button>
      </div>
      <p aria-live="polite" className="text-muted-foreground">
        {state
          ? `Input ${state.inputConnected ? "connected" : "disabled or unavailable"}; output ${state.outputConnected ? "connected" : "disabled or unavailable"}. ${state.droppedEvents} dropped events.`
          : "Loading MIDI devices…"}
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
      <p className="text-muted-foreground">
        Play notes and sustain into the chosen channel. The destination resets
        when its channel is removed or another project opens. Output forwards
        live notes, and a MIDI output channel in the rack sends its pattern.
        Right-click a knob or fader to link it to a hardware control. A
        profile maps a known surface to the mixer and the transport.
      </p>
    </FieldSet>
  )
}
