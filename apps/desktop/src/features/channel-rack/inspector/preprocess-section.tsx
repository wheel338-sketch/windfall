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
  // A new sample or a changed sampler setting starts a fresh form.
  return (
    <PreprocessForm key={JSON.stringify(channel.source)} channel={channel} />
  )
}

function PreprocessForm({ channel }: { channel: SamplerChannel }) {
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
    }
  }, [])

  const set = (patch: Partial<PreprocessChoices>) => {
    setError(null)
    setChoices((current) => ({ ...current, ...patch }))
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
      <FieldLabel htmlFor={`${id}-${name}`}>{label}</FieldLabel>
    </Field>
  )

  return (
    <Section title="Pre-process sample">
      <FieldGroup>
        {toggle("trim", "Trim to the played region", !trimmed)}
        {toggle("reverse", "Reverse the audio")}
        {toggle("removeDc", "Remove DC offset")}
        {toggle("invertPolarity", "Invert polarity")}
        {toggle("swapStereo", "Swap left and right")}
        <Field>
          <FieldLabel htmlFor={`${id}-fade-in`}>Fade in (ms)</FieldLabel>
          <Input
            id={`${id}-fade-in`}
            type="number"
            min={0}
            max={MAX_PREPROCESS_FADE_MS}
            step={1}
            placeholder="0"
            value={choices.fadeInMs}
            disabled={pending || empty}
            onChange={(event) => set({ fadeInMs: event.target.value })}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor={`${id}-fade-out`}>Fade out (ms)</FieldLabel>
          <Input
            id={`${id}-fade-out`}
            type="number"
            min={0}
            max={MAX_PREPROCESS_FADE_MS}
            step={1}
            placeholder="0"
            value={choices.fadeOutMs}
            disabled={pending || empty}
            onChange={(event) => set({ fadeOutMs: event.target.value })}
          />
        </Field>
        <Field data-disabled={!seam || undefined}>
          <Knob
            label="Loop crossfade"
            aria-label="Crossfade written into the loop seam"
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
              ? "Blends this share of the forward loop across its seam in the new sample, and sets the playback crossfade to zero."
              : "Turn on a forward loop to crossfade its seam."}
          </FieldDescription>
        </Field>
        {toggle("normalize", "Normalize to −1 dBFS")}
        <FieldDescription>
          Apply renders a new sample and gives it to this channel as one undo
          step. The original sample and its file are kept.
        </FieldDescription>
        {backend.kind === "mock" && (
          <FieldDescription>
            Browser mode cannot render a new sample. Apply will keep the
            settings and history.
          </FieldDescription>
        )}
        {error && (
          <Field data-invalid>
            <FieldError role="alert">{error}</FieldError>
          </Field>
        )}
      </FieldGroup>
      {pending && (
        <p role="status" className="flex items-center gap-2">
          <Spinner role="presentation" aria-hidden />
          Rendering the new sample…
        </p>
      )}
      <div className="flex gap-2">
        <Button
          size="sm"
          disabled={pending || empty}
          onClick={() => void apply()}
        >
          Apply to sample
        </Button>
      </div>
    </Section>
  )
}
