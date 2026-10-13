import { message, translatedText as t, useLanguageStore } from "@/lib/i18n"
import { usePluginBinding } from "@/features/plugins/store"
import { useNativeNoteCapabilities, useNoteChannels, expressionSupported } from "@/features/plugins/note-capabilities"
import { useState } from "react"
import type { Channel, ExpressionDestination, ExpressionMap, PluginExpressionDestination } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { Choice } from "@/features/export/choice"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { Section } from "./parts"

export const DEFAULT_EXPRESSION_MAP: ExpressionMap = {
  modulationX: { destination: "cutoff", plugin: "brightness", amount: 1, invert: false },
  modulationY: { destination: "resonance", plugin: "expression", amount: 1, invert: false },
  release: { destination: "release", plugin: "none", amount: 1, invert: false },
  pitchRangeCents: 1200, bendRangeSemitones: 2,
}
const BUILTIN: ExpressionDestination[] = ["none", "cutoff", "resonance", "volume", "pan", "pitch", "release"]
const PLUGIN: PluginExpressionDestination[] = ["none", "volume", "pan", "tuning", "vibrato", "expression", "brightness", "pressure"]
const ROUTES = [{ key: "modulationX", get label() { return t("Modulation X") } }, { key: "modulationY", get label() { return t("Modulation Y") } }, { key: "release", get label() { return t("Release") } }] as const
const options = <T extends string>(items: T[]) => items.map((value) => ({ value, label: t(value[0].toUpperCase() + value.slice(1)) }))

/** A reviewed, single-step edit of the channel's built-in and hosted routes. */
export function ExpressionSection({ channel }: { channel: Channel }) {
  useLanguageStore((state) => state.language)
  const binding = usePluginBinding({ type: "instrument", channel: channel.id })
  const native = useNativeNoteCapabilities(binding?.target)
  const midiChannels = useNoteChannels(binding?.target, native.capabilities)
  const revision = useProjectStore((state) => state.revision)
  const [review, setReview] = useState(() => ({ map: structuredClone(channel.expressionMap ?? DEFAULT_EXPRESSION_MAP), revision, generation: getProjectGeneration() }))
  const [pending, setPending] = useState(false)
  const [failed, setFailed] = useState(false)
  const stale = revision !== review.revision || review.generation !== getProjectGeneration()
  const map = review.map
  const pan = ROUTES.some(({ key }) => map[key].plugin === "pan")
  const volume = ROUTES.some(({ key }) => map[key].plugin === "volume")
  const refused = !!binding && (pan || volume) && midiChannels.some((channel) => !expressionSupported(native.capabilities, channel))
  const valid = ROUTES.every(({ key }) => Number.isFinite(map[key].amount) && map[key].amount >= 0 && map[key].amount <= 1)
    && Number.isFinite(map.pitchRangeCents) && map.pitchRangeCents >= 1 && map.pitchRangeCents <= 4800
    && Number.isFinite(map.bendRangeSemitones) && map.bendRangeSemitones >= 1 && map.bendRangeSemitones <= 96
  const edit = (map: ExpressionMap) => setReview({ ...review, map })
  const reload = () => { setReview({ map: structuredClone(channel.expressionMap ?? DEFAULT_EXPRESSION_MAP), revision, generation: getProjectGeneration() }); setFailed(false) }
  return <Section title={t("Note expression mapping")}>
    {(stale || failed) && <Alert><AlertDescription>{stale ? t("The channel changed. Reload the map before applying.") : t("The map could not be applied.")}</AlertDescription></Alert>}
    {refused && <Alert><AlertDescription>{t("The note capabilities of this plugin are not known yet, or a note uses an invalid MIDI channel. Apply once the instrument is running.")}</AlertDescription></Alert>}
    <FieldGroup>
      {ROUTES.map(({ key, label }) => <FieldGroup key={key}>
        <Field><FieldLabel htmlFor={`${channel.id}-${key}-built`}>{message("{label} · built-in", { label: t(label) })}</FieldLabel>
          <Choice<ExpressionDestination> id={`${channel.id}-${key}-built`} value={map[key].destination} options={options(BUILTIN)} disabled={pending || stale} onChange={(destination) => edit({ ...map, [key]: { ...map[key], destination } })} /></Field>
        <Field><FieldLabel htmlFor={`${channel.id}-${key}-plugin`}>{message("{label} · plugin", { label: t(label) })}</FieldLabel>
          <Choice<PluginExpressionDestination> id={`${channel.id}-${key}-plugin`} value={map[key].plugin} options={options(PLUGIN)} disabled={pending || stale} onChange={(plugin) => edit({ ...map, [key]: { ...map[key], plugin } })} /></Field>
        <Field><FieldLabel htmlFor={`${channel.id}-${key}-amount`}>{message("{label} depth (%)", { label: t(label) })}</FieldLabel>
          <Input id={`${channel.id}-${key}-amount`} type="number" min={0} max={100} value={Number.isFinite(map[key].amount) ? map[key].amount * 100 : ""} disabled={pending || stale} onChange={(event) => edit({ ...map, [key]: { ...map[key], amount: event.target.valueAsNumber / 100 } })} /></Field>
        <Field orientation="horizontal"><FieldLabel htmlFor={`${channel.id}-${key}-invert`}>{message("Invert {label}", { label: t(label) })}</FieldLabel>
          <Switch id={`${channel.id}-${key}-invert`} checked={map[key].invert} disabled={pending || stale} onCheckedChange={(invert) => edit({ ...map, [key]: { ...map[key], invert } })} /></Field>
      </FieldGroup>)}
      <Field><FieldLabel htmlFor={`${channel.id}-expression-range`}>{t("Mapped pitch range (cents)")}</FieldLabel>
        <Input id={`${channel.id}-expression-range`} type="number" min={1} max={4800} value={Number.isFinite(map.pitchRangeCents) ? map.pitchRangeCents : ""} disabled={pending || stale} onChange={(event) => edit({ ...map, pitchRangeCents: event.target.valueAsNumber })} /></Field>
      <Field><FieldLabel htmlFor={`${channel.id}-bend-range`}>{t("Plugin bend range (semitones)")}</FieldLabel>
        <Input id={`${channel.id}-bend-range`} type="number" min={1} max={96} value={Number.isFinite(map.bendRangeSemitones) ? map.bendRangeSemitones : ""} disabled={pending || stale} onChange={(event) => edit({ ...map, bendRangeSemitones: event.target.valueAsNumber })} /></Field>
    </FieldGroup>
    <p className="text-xs text-muted-foreground">{t("Plugin destinations use supported note expressions. MIDI-only pitch uses channel bend; set the same range in the plugin.")}</p>
    <div className="flex flex-wrap gap-2">
      <Button size="sm" variant="outline" disabled={pending} onClick={reload}>{t("Reload map")}</Button>
      <Button size="sm" variant="outline" disabled={pending || stale} onClick={() => edit(structuredClone(DEFAULT_EXPRESSION_MAP))}>{t("Default map")}</Button>
      <Button size="sm" disabled={pending || stale || !valid || refused} onClick={async () => {
        if (pending || !valid || refused || useProjectStore.getState().revision !== review.revision || getProjectGeneration() !== review.generation) return
        setPending(true)
        try { const result = await dispatch({ type: "setChannelExpressionMap", id: channel.id, map, expected: channel.expressionMap ?? DEFAULT_EXPRESSION_MAP }); setFailed(!result) } finally { setPending(false) }
      }}>{t("Apply map")}</Button>
    </div>
  </Section>
}
