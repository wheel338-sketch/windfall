import { useState } from "react"
import { create } from "zustand"
import type { ChannelId, DispatchResult } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from "@/components/ui/dialog"
import { Field, FieldGroup, FieldLabel, FieldDescription } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { backend, errorMessage } from "@/lib/ipc"
import { localizeBackendText, message, useTranslation } from "@/lib/i18n"
import { onProjectReplaced, getProjectGeneration } from "@/lib/store/replaced"
import { onHistoryNavigation, receivePatch, useProjectStore } from "@/lib/store/project"
import { DEFAULT_RENDER, UNSUPPORTED_PRESET, isProprietaryPreset,
  type PresetRenderInput, type PresetRenderSettings, type PresetRenderTicket,
  type SamplePresetRender } from "./types"

type Target = {
  input: Omit<PresetRenderInput, "options">
  settings: PresetRenderSettings
  generation: number
  ticket?: PresetRenderTicket
  error?: string
  current: () => boolean
  finish: (result: DispatchResult | null) => void
}
export const usePresetRenderDialog = create(() => ({ target: null as Target | null }))

export function closePresetRender() {
  const target = usePresetRenderDialog.getState().target
  usePresetRenderDialog.setState({ target: null })
  if (target?.ticket) void backend.presetRenderCancel(target.ticket.request).catch(() => {})
  target?.finish(null)
}
onProjectReplaced(closePresetRender)
onHistoryNavigation(closePresetRender)

/** Returns null on dismissal. The existing caller still owns patch/selection. */
export function openPresetRender(input: Omit<PresetRenderInput, "options">,
  settings = DEFAULT_RENDER, current: () => boolean = () => true): Promise<DispatchResult | null> {
  closePresetRender()
  return new Promise((finish) => {
    const target: Target = { input, settings, current, finish, generation: getProjectGeneration() }
    usePresetRenderDialog.setState({ target })
    if (isProprietaryPreset(input.path)) return
    void backend.presetRenderBegin().then((ticket) => {
      if (usePresetRenderDialog.getState().target !== target || !current()
          || target.generation !== getProjectGeneration()) {
        void backend.presetRenderCancel(ticket.request).catch(() => {})
        if (usePresetRenderDialog.getState().target === target) closePresetRender()
        return
      }
      usePresetRenderDialog.setState({ target: { ...target, ticket } })
    }).catch((error: unknown) => {
      if (usePresetRenderDialog.getState().target === target)
        usePresetRenderDialog.setState({ target: { ...target, error: errorMessage(error) } })
    })
  })
}

export function PresetRenderDialog() {
  const t = useTranslation()
  const target = usePresetRenderDialog((state) => state.target)
  return <Dialog open={target !== null} onOpenChange={(open) => { if (!open) closePresetRender() }}>
    <DialogContent>
      <DialogHeader>
        <DialogTitle>{target && isProprietaryPreset(target.input.path) ? t("Unsupported preset source") : t("Render preset as sample")}</DialogTitle>
        <DialogDescription>{t("The rendered sample and its source preset stay with the project.")}</DialogDescription>
      </DialogHeader>
      {target && <RenderForm key={`${target.generation}/${target.input.path}/${target.input.source ?? "file"}`} target={target} />}
    </DialogContent>
  </Dialog>
}

function RenderForm({ target }: { target: Target }) {
  const t = useTranslation()
  const [options, setOptions] = useState(target.settings)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const unsupported = isProprietaryPreset(target.input.path)
  const invalid = !Number.isInteger(options.note) || options.note < 0 || options.note > 127
    || !Number.isFinite(options.velocity) || options.velocity < 0 || options.velocity > 1
    || !Number.isFinite(options.lengthSecs) || options.lengthSecs < 0.01 || options.lengthSecs > 60
    || !Number.isFinite(options.tailSecs) || options.tailSecs < 0 || options.tailSecs > 30
  async function render() {
    if (!target.ticket || busy || invalid) return
    if (!target.current() || target.generation !== getProjectGeneration()) { closePresetRender(); return }
    setBusy(true)
    setError(null)
    try {
      const result = await backend.presetRenderFile(target.ticket.request, { ...target.input, options })
      if (usePresetRenderDialog.getState().target === target && target.current()
          && target.generation === getProjectGeneration()) {
        usePresetRenderDialog.setState({ target: null })
        target.finish(result)
      } else target.finish(null)
    } catch (error: unknown) {
      if (usePresetRenderDialog.getState().target === target) {
        setError(errorMessage(error))
        // A failed job consumes its lease. Reopen to capture a fresh project.
        setBusy(false)
      }
    }
  }
  return <>
    {unsupported ? <Alert variant="destructive"><AlertDescription>{t(UNSUPPORTED_PRESET)}</AlertDescription></Alert> : <FieldGroup>
      {([
        ["note", t("MIDI note"), 0, 127, 1], ["velocity", t("Velocity"), 0, 1, 0.01],
        ["lengthSecs", t("Note length (seconds)"), 0.01, 60, 0.01],
        ["tailSecs", t("Release tail limit (seconds)"), 0, 30, 0.1],
      ] as const).map(([field, label, min, max, step]) => <Field key={field} data-disabled={busy}
        data-invalid={!Number.isFinite(options[field]) || options[field] < min || options[field] > max || (field === "note" && !Number.isInteger(options[field]))}>
        <FieldLabel htmlFor={`preset-render-${field}`}>{label}</FieldLabel>
        <Input id={`preset-render-${field}`} type="number" min={min} max={max} step={step}
          disabled={busy} value={Number.isFinite(options[field]) ? options[field] : ""}
          aria-invalid={!Number.isFinite(options[field]) || options[field] < min || options[field] > max || (field === "note" && !Number.isInteger(options[field]))}
          onChange={(event) => setOptions({ ...options, [field]: event.target.valueAsNumber })} />
      </Field>)}
      <Field>
        <FieldLabel>{t("Sample rate")}</FieldLabel>
        <FieldDescription>{target.ticket ? message("{rate} Hz (project rate)", { rate: target.ticket.sampleRate }) : target.error ? t("Project rate unavailable") : t("Reading project rate…")}</FieldDescription>
      </Field>
      <Field>
        <FieldLabel>{t("Channels")}</FieldLabel>
        <ToggleGroup aria-label={t("Rendered channels")} value={[options.stereo ? "stereo" : "mono"]} disabled={busy}
          onValueChange={(values) => { if (values.length) setOptions({ ...options, stereo: values[0] === "stereo" }) }}>
          <ToggleGroupItem value="mono">{t("Mono")}</ToggleGroupItem>
          <ToggleGroupItem value="stereo">{t("Stereo")}</ToggleGroupItem>
        </ToggleGroup>
        <FieldDescription>{t("Speech keeps the complete phrase at its preset settings. Its note extends to the phrase length.")}</FieldDescription>
      </Field>
    </FieldGroup>}
    {(error || target.error) && <Alert variant="destructive"><AlertDescription>{error ? message("{error} Close and reopen this dialog to retry.", { error: localizeBackendText(error) }) : localizeBackendText(target.error!)}</AlertDescription></Alert>}
    {busy && <p role="status">{t("Rendering instrument audio…")}</p>}
    <div className="flex justify-end gap-2">
      <Button variant="outline" onClick={closePresetRender}>{busy ? t("Cancel rendering") : t("Cancel")}</Button>
      {!unsupported && <Button onClick={() => void render()} disabled={busy || invalid || !target.ticket || error !== null}>{t("Render as sample")}</Button>}
    </div>
  </>
}

export function PresetRerenderButton({ channel }: { channel: ChannelId }) {
  const t = useTranslation()
  const project = useProjectStore((state) => state.project)
  const sample = project.channels.find((c) => c.id === channel)?.source
  const id = sample?.type === "sampler" ? sample.sample : null
  const recipe = (project.settings as typeof project.settings & { presetRenders?: SamplePresetRender[] }).presetRenders?.find((r) => r.sample === id)
  const source = recipe && project.samples.find((s) => s.id === recipe.source)
  if (!recipe || !source) return null
  return <Button size="sm" variant="outline" onClick={() => {
    const generation = getProjectGeneration()
    void openPresetRender({ path: source.path.path, source: recipe.source, channel }, recipe.settings,
      () => generation === getProjectGeneration()).then((result) => { if (result) receivePatch(result.patch) })
  }}>{t("Re-render preset…")}</Button>
}
