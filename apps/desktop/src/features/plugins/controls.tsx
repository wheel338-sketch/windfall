import { useEffect, useId, useState } from "react"
import type {
  AutomationTarget,
  PluginBinding,
  PluginParameter,
} from "@/bindings"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { ValueContextItems } from "@/components/value-context-menu"
import { useAutomation } from "@/features/automation/live"
import { useGestureValue } from "@/features/mixer/use-gesture-value"
import { reportError } from "@/lib/errors"
import { backend } from "@/lib/ipc"
import { dispatch, useProjectStore } from "@/lib/store"
import { openPluginManager, sameTarget } from "./store"
import { PluginWrapperControls } from "./wrapper-controls"

function Parameter({
  binding,
  parameter,
  index,
}: {
  binding: PluginBinding
  parameter: PluginParameter
  index: number
}) {
  const unavailable = binding.capabilities?.unavailableParameters.includes(parameter.id) ?? false
  const id = useId()
  const track = useProjectStore(
    (state) =>
      state.project.mixer.tracks.find((track) =>
        track.effects.some(
          (effect) =>
            binding.target.type === "effect" &&
            effect.id === binding.target.effect
        )
      )?.id ?? 0
  )
  const target: AutomationTarget =
    binding.target.type === "instrument"
      ? {
          type: "instrumentParam",
          channel: binding.target.channel,
          param: index,
        }
      : {
          type: "effectParam",
          track,
          effect: binding.target.effect,
          param: index,
        }
  const automation = useAutomation(target)
  const value = useGestureValue(
    parameter.value,
    (value) => ({
      type: "setPluginParam",
      target: binding.target,
      id: parameter.id,
      value,
    }),
    (value) =>
      Math.min(
        parameter.max,
        Math.max(parameter.min, parameter.stepped ? Math.round(value) : value)
      )
  )
  return (
    <Field
      orientation="horizontal"
      data-disabled={parameter.readOnly || unavailable || undefined}
    >
      <FieldLabel htmlFor={id}>{parameter.name}</FieldLabel>
      <ValueContextItems
        items={
          parameter.automatable && !parameter.readOnly && !unavailable ? automation.items : []
        }
      >
        <Input
          id={id}
          type="number"
          min={parameter.min}
          max={parameter.max}
          step={parameter.stepped ? 1 : "any"}
          disabled={parameter.readOnly || unavailable}
          value={value.value}
          onFocus={value.onGestureStart}
          onBlur={value.onGestureEnd}
          onChange={(event) => {
            const next = Number(event.target.value)
            if (Number.isFinite(next)) value.onValueChange(next)
          }}
        />
      </ValueContextItems>
    </Field>
  )
}
export function PluginControls({ binding }: { binding: PluginBinding }) {
  const tracks = useProjectStore((state) => state.project.mixer.tracks)
  const capabilities = binding.capabilities
  const outputs = [...(capabilities?.outputs ?? []), ...(capabilities?.routes ?? []).filter((route) => !(capabilities?.outputs ?? []).some((port) => port.index === route.output)).map((route) => ({ index: route.output, name: `Saved output ${route.output + 1} (unavailable)`, channels: 0 }))]
  const diagnostics = [
    ...(capabilities?.unavailableParameters ?? []).map((id) => `Parameter ${id} is unavailable. Its automation and controller links are retained and will resume when it returns.`),
    ...(capabilities?.routes ?? []).filter((route) => !(capabilities?.outputs ?? []).some((port) => port.index === route.output) || !tracks.some((track) => track.id === route.track)).map((route) => `Saved output ${route.output + 1} to mixer track ${route.track} is unavailable. The route is retained.`),
  ]
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let cancelled = false
    const refresh = () =>
      backend
        .pluginsState()
        .then((state) => {
          if (!cancelled)
            setError(
              state.instances.find((instance) =>
                sameTarget(instance.target, binding.target)
              )?.error ?? null
            )
        })
        .catch((error: unknown) => {
          if (!cancelled)
            setError(error instanceof Error ? error.message : String(error))
        })
    void refresh()
    const timer = window.setInterval(() => void refresh(), 1000)
    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [binding.target])
  const editor = (open: boolean) =>
    backend
      .pluginEditor(binding.target, open)
      .catch((error: unknown) =>
        reportError(error, "Could not change the plugin editor")
      )
  return (
    <div className="flex flex-col gap-3">
      <p className="text-muted-foreground">
        {binding.format.toUpperCase()} · {binding.id}
      </p>
      {error && (
        <Alert>
          <AlertTitle>Plugin unavailable</AlertTitle>
          <AlertDescription>
            {error}.{" "}
            {binding.target.type === "instrument"
              ? "This channel is silent."
              : "This effect is bypassed."}{" "}
            Its reference and saved state are retained.
          </AlertDescription>
        </Alert>
      )}
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled={!!error || backend.kind === "mock"}
          onClick={() => void editor(true)}
        >
          Open native editor
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={backend.kind === "mock"}
          onClick={() => void editor(false)}
        >
          Close native editor
        </Button>
        <Button size="sm" variant="outline" onClick={() => openPluginManager()}>
          Manage plugins
        </Button>
      </div>
      {diagnostics.length > 0 && <Alert><AlertTitle>Plugin capabilities changed</AlertTitle><AlertDescription>{diagnostics.map((diagnostic) => <p key={diagnostic}>{diagnostic}</p>)}</AlertDescription></Alert>}
      <PluginWrapperControls binding={binding} />
      <FieldGroup>
        {outputs.map((port) => {
          const route = capabilities?.routes.find((route) => route.output === port.index)
          const items = [{ value: -1, label: "Disconnected" }, ...tracks.filter((track) => !track.current).map((track) => ({ value: track.id, label: track.name }))]
          return <Field key={port.index}><FieldLabel>{port.name || `Output ${port.index + 1}`}</FieldLabel>
            <Select value={route?.track ?? -1} items={items} onValueChange={(track: number | null) => {
              if (track !== null) void dispatch({ type: "setPluginOutputRoute", target: binding.target, output: port.index, track: track < 0 ? null : track }).catch((error: unknown) => reportError(error, "Could not route the native output"))
            }}>
              <SelectTrigger aria-label={`Route plugin output ${port.index + 1}`}><SelectValue /></SelectTrigger>
              <SelectContent><SelectGroup>{items.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}</SelectGroup></SelectContent>
            </Select>
          </Field>
        })}
      </FieldGroup>
      <FieldGroup>
        {binding.target.type === "effect" && (
          <Field>
            <FieldLabel>Sidechain input</FieldLabel>
            <Select
              value={binding.sidechainInput ?? -1}
              items={[
                { value: -1, label: "First auxiliary input" },
                ...(binding.auxiliaryInputs ?? []).map((input) => ({ value: input.index, label: `${input.name || `Input ${input.index + 1}`} · ${input.channels === 1 ? "Mono" : input.channels === 2 ? "Stereo" : `${input.channels} channels`}` })),
                ...(binding.sidechainInput != null && !(binding.auxiliaryInputs ?? []).some((input) => input.index === binding.sidechainInput) ? [{ value: binding.sidechainInput, label: `Saved input ${binding.sidechainInput + 1} (unavailable)` }] : []),
              ]}
              onValueChange={(input: number | null) => {
                if (input !== null) void dispatch({ type: "setPluginSidechainInput", target: binding.target, input: input < 0 ? null : input }).catch((error: unknown) => reportError(error, "Could not choose the plugin sidechain input"))
              }}
            >
              <SelectTrigger aria-label="Plugin sidechain input"><SelectValue /></SelectTrigger>
              <SelectContent><SelectGroup>
                <SelectItem value={-1}>First auxiliary input</SelectItem>
                {(binding.auxiliaryInputs ?? []).map((input) => <SelectItem key={input.index} value={input.index}>{input.name || `Input ${input.index + 1}`} · {input.channels === 1 ? "Mono" : input.channels === 2 ? "Stereo" : `${input.channels} channels`}</SelectItem>)}
                {binding.sidechainInput != null && !(binding.auxiliaryInputs ?? []).some((input) => input.index === binding.sidechainInput) && <SelectItem value={binding.sidechainInput} disabled>Saved input {binding.sidechainInput + 1} (unavailable)</SelectItem>}
              </SelectGroup></SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">Receives this track’s detector-only sends. {!(binding.auxiliaryInputs ?? []).length && "No auxiliary inputs were reported when this plugin was added."}</p>
          </Field>
        )}
        {binding.parameters.map((parameter, index) => (
          <Parameter
            key={parameter.id}
            binding={binding}
            parameter={parameter}
            index={index}
          />
        ))}
      </FieldGroup>
    </div>
  )
}
