import { localizeBackendText } from "@/lib/i18n"
import { message as i18nMessage } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import { useEffect, useId, useRef, useState } from "react"

import { Knob, percentUnit } from "@/components/audio"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import type { SamplerChannel } from "@/lib/channel-source"
import { backend } from "@/lib/ipc"
import { receivePatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"

import { Section } from "./parts"
import {
  hasLoopSeam,
  isTrimmed,
  MAX_PREPROCESS_CROSSFADE,
  MAX_PREPROCESS_FADE_MS,
  NO_PREPROCESSING,
  preprocessRequest,
  type PreprocessChoices,
} from "./preprocess"

type Toggle =
  | "trim"
  | "reverse"
  | "removeDc"
  | "invertPolarity"
  | "swapStereo"
  | "normalize"

function toggled(name: Toggle, on: boolean): Partial<PreprocessChoices> {
  const patch: Partial<Record<Toggle, boolean>> = {}
  patch[name] = on
  return patch
}

/**
 * Offline treatments that render a new sample: trim, reverse, DC removal,
 * polarity, channel swap, fades, a crossfaded loop seam and normalizing.
 * The original sample and its file are kept; Apply is one undo step.
 */
export function SamplePreprocessSection({
  channel,
}: {
  channel: SamplerChannel
}) {
  useLanguageStore((state) => state.language)
  // A new sample or a changed sampler setting starts a fresh form.
  return (
    <PreprocessForm key={JSON.stringify(channel.source)} channel={channel} />
  )
}

function PreprocessForm({ channel }: { channel: SamplerChannel }) {
  useLanguageStore((state) => state.language)
  const [preview, setPreview] = useState<import("@/bindings").SampleInfo | null>(null)
  const [collected, setCollected] = useState<number | null>(null)
  const [choices, setChoices] = useState<PreprocessChoices>(NO_PREPROCESSING)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const mounted = useRef(true)
  const id = useId()
  const seam = hasLoopSeam(channel)
  const trimmed = isTrimmed(channel)
  const empty = channel.source.sample === null

  useEffect(() => {
    mounted.current = true
    // A patch or a project replacement makes a result in flight stale; the
    // desktop refuses it, and this form no longer waits for it.
    const unsubscribe = useProjectStore.subscribe((state, old) => {
      if (state.project !== old.project && mounted.current) setPending(false)
    })
    return () => {
      mounted.current = false
      unsubscribe()
      void backend.previewStop().catch(() => {})
    }
  }, [])

  const set = (patch: Partial<PreprocessChoices>) => {
    setError(null)
    setPreview(null)
    void backend.previewStop().catch(() => {})
    setChoices((current) => ({ ...current, ...patch }))
  }

  async function audition() {
    const built = preprocessRequest(channel, choices)
    if ("error" in built) { setError(built.error); return }
    const generation = getProjectGeneration()
    setPending(true)
    setError(null)
    try {
      const result = await backend.samplerPreprocessPreview(built.request)
      if (mounted.current && generation === getProjectGeneration()) setPreview(result)
    } catch (reason) {
      if (mounted.current && generation === getProjectGeneration())
        setError(reason instanceof Error ? reason.message : String(reason))
    } finally { if (mounted.current) setPending(false) }
  }

  async function collect() {
    setPending(true)
    setError(null)
    try {
      const count = await backend.collectDerivedAssets()
      if (mounted.current) setCollected(count)
    } catch (reason) {
      if (mounted.current) setError(reason instanceof Error ? reason.message : String(reason))
    } finally { if (mounted.current) setPending(false) }
  }

  async function apply() {
    const built = preprocessRequest(channel, choices)
    if ("error" in built) {
      setError(built.error)
      return
    }
    setError(null)
    setPending(true)
    const generation = getProjectGeneration()
    try {
      const result = await backend.samplerPreprocess(built.request)
      if (generation === getProjectGeneration()) receivePatch(result.patch)
    } catch (reason) {
      if (mounted.current && generation === getProjectGeneration())
        setError(reason instanceof Error ? reason.message : String(reason))
    } finally {
      if (mounted.current) setPending(false)
    }
  }

  const toggle = (name: Toggle, label: string, disabled = false) => (
    <Field orientation="horizontal" data-disabled={disabled || undefined}>
      <Checkbox
        id={`${id}-${name}`}
        checked={choices[name]}
        disabled={pending || empty || disabled}
        onCheckedChange={(checked) => set(toggled(name, checked === true))}
      />
      <FieldLabel htmlFor={`${id}-${name}`}>{t(label)}</FieldLabel>
    </Field>
  )

  return (
    <Section title={t("Pre-process sample")}>
      <FieldGroup>
        {toggle("trim", t("Trim to the played region"), !trimmed)}
        {toggle("reverse", t("Reverse the audio"))}
        {toggle("removeDc", t("Remove DC offset"))}
        {toggle("invertPolarity", t("Invert polarity"))}
        {toggle("swapStereo", t("Swap left and right"))}
        <Field>
          <FieldLabel htmlFor={`${id}-fade-in`}>{t("Fade in (ms)")}</FieldLabel>
          <Input
            id={`${id}-fade-in`}
            type="number"
            min={0}
            max={MAX_PREPROCESS_FADE_MS}
            step={1}
            placeholder={t("0")}
            value={choices.fadeInMs}
            disabled={pending || empty}
            onChange={(event) => set({ fadeInMs: event.target.value })}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor={`${id}-fade-out`}>{t("Fade out (ms)")}</FieldLabel>
          <Input
            id={`${id}-fade-out`}
            type="number"
            min={0}
            max={MAX_PREPROCESS_FADE_MS}
            step={1}
            placeholder={t("0")}
            value={choices.fadeOutMs}
            disabled={pending || empty}
            onChange={(event) => set({ fadeOutMs: event.target.value })}
          />
        </Field>
        <Field data-disabled={!seam || undefined}>
          <Knob
            label={t("Loop crossfade")}
            aria-label={t("Crossfade written into the loop seam")}
            value={choices.loopCrossfade}
            onValueChange={(loopCrossfade) => set({ loopCrossfade })}
            min={0}
            max={MAX_PREPROCESS_CROSSFADE}
            step={0.01}
            defaultValue={0}
            showValue
            disabled={pending || empty || !seam}
            {...percentUnit}
          />
          <FieldDescription>
            {seam
              ? t("Blends this share of the forward loop across its seam in the new sample, and sets the playback crossfade to zero.")
              : t("Turn on a forward loop to crossfade its seam.")}
          </FieldDescription>
        </Field>
        {toggle("normalize", t("Normalize to −1 dBFS"))}
        <FieldDescription>{t("Apply renders a new sample and gives it to this channel as one undo step. The original sample and its file are kept.")}</FieldDescription>
        {backend.kind === "mock" && (
          <FieldDescription>{t("Browser mode cannot render a new sample. Apply will keep the settings and history.")}</FieldDescription>
        )}
        {error && (
          <Field data-invalid>
            <FieldError role="alert">{localizeBackendText(error)}</FieldError>
          </Field>
        )}
      </FieldGroup>
      {pending && (
        <p role="status" className="flex items-center gap-2">
          <Spinner role="presentation" aria-hidden />{t("Rendering the new sample…")}</p>
      )}
      <div className="flex gap-2">
        <Button size="sm" variant="outline" disabled={pending || empty} onClick={() => void audition()}>{t("Preview prepared sample")}</Button>
        <Button
          size="sm"
          disabled={pending || empty}
          onClick={() => void apply()}
        >{t("Apply to sample")}</Button>
      </div>
      {preview && <div>
        <output aria-label={t("Prepared sample duration")}>{i18nMessage("{frames} frames · {seconds} s · {rate} Hz", { frames: preview.frames, seconds: preview.durationSecs.toFixed(3), rate: preview.sampleRate })}</output>
        <svg role="img" aria-label={t("Prepared sample waveform")} viewBox="0 0 512 100" className="h-24 w-full">
          <path stroke="currentColor" d={preview.peaks.map((value, i) => `${i % 2 ? "L" : "M"}${Math.floor(i / 2) * 512 / Math.max(1, preview.peaks.length / 2 - 1)},${50 - value * 45}`).join(" ")} />
        </svg>
        <Button size="sm" variant="outline" onClick={() => void backend.previewStop()}>{t("Stop prepared preview")}</Button>
      </div>}
      <Button size="sm" variant="ghost" disabled={pending} onClick={() => void collect()}>{t("Collect unused sample edits")}</Button>
      <p className="text-xs text-muted-foreground">{t("Cleanup retains current sources, undo/redo and saved assets. Only files created in this session are eligible.")}</p>
      {collected !== null && <output aria-label={t("Collected sample edits")}>{i18nMessage("Removed unused sample files", { count: collected })}</output>}
    </Section>
  )
}
