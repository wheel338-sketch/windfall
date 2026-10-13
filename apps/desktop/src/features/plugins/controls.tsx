import { localizeBackendText } from "@/lib/i18n"
import { message as i18nMessage } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
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
  useLanguageStore((state) => state.language)
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
      data-disabled={parameter.readOnly || undefined}
    >
      <FieldLabel htmlFor={id}>{parameter.name}</FieldLabel>
      <ValueContextItems
        items={
          parameter.automatable && !parameter.readOnly ? automation.items : []
        }
      >
        <Input
          id={id}
          type="number"
          min={parameter.min}
          max={parameter.max}
          step={parameter.stepped ? 1 : "any"}
          disabled={parameter.readOnly}
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
  useLanguageStore((state) => state.language)
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
        reportError(error, t("Could not change the plugin editor"))
      )
  return (
    <div className="flex flex-col gap-3">
      <p className="text-muted-foreground">
        {binding.format.toUpperCase()} · {binding.id}
      </p>
      {error && (
        <Alert>
          <AlertTitle>{t("Plugin unavailable")}</AlertTitle>
          <AlertDescription>
            {localizeBackendText(error)}.{" "}
            {binding.target.type === "instrument"
              ? t("This channel is silent.")
              : t("This effect is bypassed.")}{" "}
            {t("Its reference and saved state are retained.")}
          </AlertDescription>
        </Alert>
      )}
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled={!!error || backend.kind === "mock"}
          onClick={() => void editor(true)}
        >{t("Open native editor")}</Button>
        <Button
          size="sm"
          variant="outline"
          disabled={backend.kind === "mock"}
          onClick={() => void editor(false)}
        >{t("Close native editor")}</Button>
        <Button size="sm" variant="outline" onClick={() => openPluginManager()}>{t("Manage plugins")}</Button>
      </div>
      <PluginWrapperControls binding={binding} />
      <FieldGroup>
        {binding.target.type === "effect" && (
          <Field>
            <FieldLabel>{t("Sidechain input")}</FieldLabel>
            <Select
              value={binding.sidechainInput ?? -1}
              items={[
                { value: -1, label: t("First auxiliary input") },
                ...(binding.auxiliaryInputs ?? []).map((input) => ({ value: input.index, label: i18nMessage("{input} · {channels} channels", { input: input.name || i18nMessage("Input {value0}", { value0: input.index + 1 }), channels: input.channels }) })),
                ...(binding.sidechainInput != null && !(binding.auxiliaryInputs ?? []).some((input) => input.index === binding.sidechainInput) ? [{ value: binding.sidechainInput, label: i18nMessage("Saved input {value0} (unavailable)", { value0: binding.sidechainInput + 1 }) }] : []),
              ]}
              onValueChange={(input: number | null) => {
                if (input !== null) void dispatch({ type: "setPluginSidechainInput", target: binding.target, input: input < 0 ? null : input }).catch((error: unknown) => reportError(error, t("Could not choose the plugin sidechain input")))
              }}
            >
              <SelectTrigger aria-label={t("Plugin sidechain input")}><SelectValue /></SelectTrigger>
              <SelectContent><SelectGroup>
                <SelectItem value={-1}>{t("First auxiliary input")}</SelectItem>
                {(binding.auxiliaryInputs ?? []).map((input) => <SelectItem key={input.index} value={input.index}>{input.name || i18nMessage("Input {value0}", { value0: input.index + 1 })} · {input.channels === 1 ? t("Mono") : input.channels === 2 ? t("Stereo") : i18nMessage("{count} channels", { count: input.channels })}</SelectItem>)}
                {binding.sidechainInput != null && !(binding.auxiliaryInputs ?? []).some((input) => input.index === binding.sidechainInput) && <SelectItem value={binding.sidechainInput} disabled>{i18nMessage("Saved input {value0} (unavailable)", { value0: binding.sidechainInput + 1 })}</SelectItem>}
              </SelectGroup></SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">{t("Receives this track’s detector-only sends.")}{!(binding.auxiliaryInputs ?? []).length && t("No auxiliary inputs were reported when this plugin was added.")}</p>
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
