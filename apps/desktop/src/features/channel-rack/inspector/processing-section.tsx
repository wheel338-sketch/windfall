import { useEffect, useId, useRef, useState } from "react"
import type { ClipStretchQuality, SamplerStretch } from "@/bindings"
import { Knob, noteName } from "@/components/audio"
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
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import type { SamplerChannel } from "@/lib/channel-source"
import { backend } from "@/lib/ipc"
import { dispatch, receivePatch, useProjectStore } from "@/lib/store/project"
import { useSample, useTempo } from "@/lib/store/selectors"
import { Section } from "./parts"
import { useSampleInfo } from "./sample-info"
import { nextSamplerQuality } from "./sampler-quality-step"
import {
  regionSeconds,
  suggestedBeats,
  tempoFitRatio,
  tempoFitTune,
} from "./tempo-fit"

export function SamplerProcessingSection({
  channel,
}: {
  channel: SamplerChannel
}) {
  // Musical edits/undo reset the draft without writing the shared project store.
  return (
    <ProcessingForm key={JSON.stringify(channel.source)} channel={channel} />
  )
}

function ProcessingForm({ channel }: { channel: SamplerChannel }) {
  const initial = channel.source.stretch
  const spectral = initial?.mode === "spectral" ? initial : null
  const [mode, setMode] = useState<"tape" | "spectral">(initial?.mode ?? "tape")
  const [ratio, setRatio] = useState(spectral?.ratio ?? 1)
  const [quality, setQuality] = useState<ClipStretchQuality>(
    spectral?.quality ?? "standard"
  )
  const [formants, setFormants] = useState(spectral?.formants ?? false)
  const low = Math.min(116, Math.max(0, channel.source.rootKey - 6))
  const [first, setFirst] = useState(String(spectral?.range.first ?? low))
  const [last, setLast] = useState(String(spectral?.range.last ?? low + 11))
  const [pending, setPending] = useState(false)
  const [beats, setBeats] = useState("")
  const tempo = useTempo()
  const sampleInfo = useSampleInfo(useSample(channel.source.sample))
  const seconds =
    sampleInfo?.status === "ready"
      ? regionSeconds(
          sampleInfo.info.durationSecs,
          channel.source.start,
          channel.source.end
        )
      : 0
  const suggested = suggestedBeats(seconds, tempo)
  const [progress, setProgress] = useState("Preparing key variants…")
  const [error, setError] = useState<string | null>(null)
  const job = useRef<{ request: number | null; cancelled: boolean } | null>(
    null
  )
  const id = useId()

  useEffect(() => {
    const cancel = () => {
      const current = job.current
      if (current) {
        current.cancelled = true
        if (current.request !== null)
          void backend.samplerPreparationCancel(current.request)
        job.current = null
      }
    }
    // A patch or project replacement invalidates the native snapshot too.
    const unsubscribe = useProjectStore.subscribe((state, old) => {
      if (state.project !== old.project) {
        cancel()
        setPending(false)
      }
    })
    return () => {
      unsubscribe()
      cancel()
    }
  }, [])

  async function apply() {
    setError(null)
    const range = { first: Number(first), last: Number(last) }
    if (
      mode === "spectral" &&
      (!Number.isInteger(range.first) ||
        first.trim() === "" ||
        last.trim() === "" ||
        !Number.isInteger(range.last) ||
        range.first < 0 ||
        range.last > 127 ||
        range.first > range.last)
    ) {
      setError("Choose an ordered MIDI key range from 0 to 127.")
      return
    }
    const stretch: SamplerStretch =
      mode === "tape" ? { mode } : { mode, ratio, quality, formants, range }
    if (mode === "tape") {
      await dispatch({
        type: "updateSampler",
        id: channel.id,
        patch: { stretch },
      })
      return
    }
    const current = { request: null as number | null, cancelled: false }
    job.current = current
    setPending(true)
    setProgress("Preparing key variants…")
    let timer: ReturnType<typeof setInterval> | undefined
    try {
      current.request = await backend.samplerPreparationBegin()
      if (current.cancelled) {
        await backend.samplerPreparationCancel(current.request)
        return
      }
      timer = setInterval(() => {
        if (current.request === null) return
        void backend
          .samplerPreparationProgress(current.request)
          .then((value) => {
            if (!current.cancelled && value.total > 0)
              setProgress(
                `Preparing key variants: ${value.completed}/${value.total}`
              )
          })
          .catch(() => {})
      }, 200)
      const result = await backend.prepareSamplerCommand(
        { type: "updateSampler", id: channel.id, patch: { stretch } },
        current.request
      )
      if (!current.cancelled) receivePatch(result.patch)
    } catch (reason) {
      if (!current.cancelled)
        setError(reason instanceof Error ? reason.message : String(reason))
    } finally {
      clearInterval(timer)
      if (job.current === current) {
        job.current = null
        setPending(false)
      }
    }
  }

  /** Makes the played region last a whole number of beats at the tempo. */
  async function fitToTempo() {
    setError(null)
    const wanted = beats.trim() === "" ? suggested : Number(beats)
    if (seconds <= 0) {
      setError("Load a sample before fitting it to the tempo.")
      return
    }
    if (!Number.isFinite(wanted) || wanted <= 0) {
      setError("Enter how many beats the sample should last.")
      return
    }
    if (mode === "spectral") {
      const fitted = tempoFitRatio(seconds, wanted, tempo)
      if (fitted === null) {
        setError(
          "That many beats needs a duration ratio outside 0.25 to 4. Choose another beat count."
        )
        return
      }
      // A draft like the knob's: Apply stretch prepares and saves it.
      setRatio(fitted)
      return
    }
    const tune = tempoFitTune(seconds, wanted, tempo)
    if (tune === null) {
      setError(
        "That many beats needs more than 48 semitones of tuning. Choose another beat count."
      )
      return
    }
    await dispatch({ type: "updateSampler", id: channel.id, patch: { tune } })
  }

  const published = spectral
    ? `Key range ${noteName(spectral.range.first)}–${noteName(spectral.range.last)} (MIDI ${spectral.range.first}–${spectral.range.last}). Other keys are silent.`
    : "Tape: pitch changes playback speed."
  return (
    <Section title="Stretch">
      <FieldGroup>
        <Field>
          <ToggleGroup
            aria-label="Sampler stretch mode"
            value={[mode]}
            disabled={pending}
            variant="outline"
            size="sm"
            spacing={0}
            onValueChange={(values) => {
              if (values[0] === "tape" || values[0] === "spectral")
                setMode(values[0])
            }}
          >
            <ToggleGroupItem value="tape">Tape</ToggleGroupItem>
            <ToggleGroupItem value="spectral">Independent</ToggleGroupItem>
          </ToggleGroup>
          <FieldDescription>{published}</FieldDescription>
        </Field>
        {mode === "spectral" && (
          <>
            <Field data-disabled={pending}>
              <Knob
                label="Duration ratio"
                value={ratio}
                onValueChange={setRatio}
                min={0.25}
                max={4}
                step={0.01}
                defaultValue={1}
                showValue
                disabled={pending}
                format={(value) => `${value.toFixed(2)}×`}
              />
              <FieldDescription>
                Duration stays the same across the prepared keys.
              </FieldDescription>
            </Field>
            <Field>
              <FieldLabel>Preparation quality</FieldLabel>
              <ToggleGroup
                aria-label="Sampler preparation quality"
                value={[quality]}
                disabled={pending}
                variant="outline"
                size="sm"
                onValueChange={(values) => {
                  if (
                    values[0] === "fast" ||
                    values[0] === "standard" ||
                    values[0] === "high"
                  )
                    setQuality(values[0])
                }}
              >
                <ToggleGroupItem value="fast">Fast</ToggleGroupItem>
                <ToggleGroupItem value="standard">Standard</ToggleGroupItem>
                <ToggleGroupItem value="high">High</ToggleGroupItem>
              </ToggleGroup>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  aria-label="Choose the previous sampler preparation quality"
                  disabled={
                    pending || nextSamplerQuality(quality, "previous") === null
                  }
                  onClick={() => {
                    const next = nextSamplerQuality(quality, "previous")
                    if (next !== null) setQuality(next)
                  }}
                >
                  Previous
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  aria-label="Choose the next sampler preparation quality"
                  disabled={
                    pending || nextSamplerQuality(quality, "next") === null
                  }
                  onClick={() => {
                    const next = nextSamplerQuality(quality, "next")
                    if (next !== null) setQuality(next)
                  }}
                >
                  Next
                </Button>
              </div>
            </Field>
            <Field orientation="horizontal">
              <Checkbox
                id={`${id}-formants`}
                checked={formants}
                disabled={pending}
                onCheckedChange={(checked) => setFormants(checked === true)}
              />
              <FieldLabel htmlFor={`${id}-formants`}>
                Preserve formants (approximate)
              </FieldLabel>
            </Field>
            <Field>
              <FieldLabel htmlFor={`${id}-first`}>
                First prepared MIDI key
              </FieldLabel>
              <Input
                id={`${id}-first`}
                type="number"
                min={0}
                max={127}
                step={1}
                value={first}
                disabled={pending}
                onChange={(event) => setFirst(event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor={`${id}-last`}>
                Last prepared MIDI key
              </FieldLabel>
              <Input
                id={`${id}-last`}
                type="number"
                min={0}
                max={127}
                step={1}
                value={last}
                disabled={pending}
                onChange={(event) => setLast(event.target.value)}
              />
              <FieldDescription>
                Every key is prepared before Apply completes. The 256 MiB
                aggregate budget includes banks held by old voices.
              </FieldDescription>
            </Field>
            {backend.kind === "mock" && (
              <FieldDescription>
                Browser mode cannot prepare or audition spectral key variants.
                Apply will keep settings and history.
              </FieldDescription>
            )}
          </>
        )}
        <Field data-disabled={pending || undefined}>
          <FieldLabel htmlFor={`${id}-beats`}>Beats to fit</FieldLabel>
          <div className="flex gap-2">
            <Input
              id={`${id}-beats`}
              type="number"
              min={0}
              step={0.25}
              placeholder={String(suggested)}
              value={beats}
              disabled={pending}
              onChange={(event) => setBeats(event.target.value)}
            />
            <Button
              variant="outline"
              size="sm"
              disabled={pending}
              onClick={() => void fitToTempo()}
            >
              Fit to tempo
            </Button>
          </div>
          <FieldDescription>
            {mode === "spectral"
              ? "Sets the duration ratio so the played region lasts that many beats at the project tempo. Pitch stays; Apply stretch prepares it."
              : "Tunes the sample so the played region lasts that many beats at its root key. Tape changes pitch with speed."}
          </FieldDescription>
        </Field>
        {error && (
          <Field data-invalid>
            <FieldError role="alert">{error}</FieldError>
          </Field>
        )}
      </FieldGroup>
      {pending && (
        <p role="status" className="flex items-center gap-2">
          <Spinner role="presentation" aria-hidden />
          {progress}
        </p>
      )}
      <div className="flex gap-2">
        <Button size="sm" disabled={pending} onClick={() => void apply()}>
          Apply stretch
        </Button>
        {pending && (
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              const current = job.current
              if (current) {
                current.cancelled = true
                if (current.request !== null)
                  void backend.samplerPreparationCancel(current.request)
                job.current = null
              }
              setPending(false)
            }}
          >
            Cancel preparation
          </Button>
        )}
      </div>
    </Section>
  )
}
