import {
  createContext,
  useContext,
  useId,
  useMemo,
  useRef,
  useState,
} from "react"

import { Knob, msUnit, percentUnit, powerScale } from "@/components/audio"
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from "@/components/ui/field"
import { Switch } from "@/components/ui/switch"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

import {
  createDefaultChannelVoiceSettings,
  LFO_TARGETS,
  MAX_VOICE_CUTOFF_HZ,
  MIN_VOICE_CUTOFF_HZ,
  resonanceEnvelopeOf,
  sanitizeChannelVoiceSettings,
  targetLfosOf,
  voiceFilterOf,
  type ChannelVoiceSettings,
  type LfoShape,
  type ModulationEnvelope,
  type NoteDivision,
  type TargetLfo,
  type TargetLfoName,
  type VoiceFilter,
  type VoiceFilterMode,
} from "./settings"

/** The targets that have an envelope in this panel. */
type EnvelopeTarget = "filter" | "pitch" | "pan" | "resonance"

type GestureHandlers = {
  onGestureStart?: () => void
  onGestureEnd?: () => void
}
/** Every knob of the panel reports the start and end of a drag through this. */
const GestureContext = createContext<GestureHandlers>({})

const SHAPES: readonly { value: LfoShape; label: string }[] = [
  { value: "sine", label: "Sine" },
  { value: "triangle", label: "Triangle" },
  { value: "square", label: "Square" },
]
const FILTER_MODES: readonly { value: VoiceFilterMode; label: string }[] = [
  { value: "lowPass", label: "Low-pass" },
  { value: "highPass", label: "High-pass" },
  { value: "bandPass", label: "Band-pass" },
  { value: "notch", label: "Notch" },
]
const LFO_FULL_DEPTH: Record<TargetLfoName, string> = {
  volume: "down to silence",
  pan: "full pan travel",
  pitch: "2400 cents",
  cutoff: "8 octaves",
  resonance: "the whole resonance range",
}

const ENVELOPE_DEPTH: Record<EnvelopeTarget, { max: number; unit: string }> =
  {
    filter: { max: 8, unit: "oct" },
    pitch: { max: 2400, unit: "ct" },
    pan: { max: 1, unit: "pan" },
    resonance: { max: 1, unit: "res" },
  }

const TIME_SCALE = powerScale(3)
const DIVISIONS: readonly { value: NoteDivision; label: string }[] = [
  { value: "quarter", label: "1/4" },
  { value: "eighth", label: "1/8" },
  { value: "sixteenth", label: "1/16" },
  { value: "thirtySecond", label: "1/32" },
]

function Choices<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string
  value: T
  options: readonly { value: T; label: string }[]
  onChange(value: T): void
}) {
  const id = useId()
  return (
    <Field>
      <FieldLabel id={id}>{label}</FieldLabel>
      <ToggleGroup
        aria-labelledby={id}
        value={[value]}
        variant="outline"
        size="sm"
        spacing={0}
        className="flex-wrap"
        onValueChange={(values) => {
          const selected = options.find((option) => option.value === values[0])
          if (selected) onChange(selected.value)
        }}
      >
        {options.map((option) => (
          <ToggleGroupItem key={option.value} value={option.value}>
            {option.label}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
    </Field>
  )
}

function Enabled({
  label,
  checked,
  onChange,
}: {
  label: string
  checked: boolean
  onChange(value: boolean): void
}) {
  const id = useId()
  return (
    <Field orientation="horizontal">
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Switch id={id} size="sm" checked={checked} onCheckedChange={onChange} />
    </Field>
  )
}

function EnvelopeControls({
  target,
  value,
  onChange,
}: {
  target: EnvelopeTarget
  value: ModulationEnvelope
  onChange(value: ModulationEnvelope): void
}) {
  const gestures = useContext(GestureContext)
  const title = `${target[0].toUpperCase()}${target.slice(1)} envelope`
  const { max: maxDepth, unit: depthUnit } = ENVELOPE_DEPTH[target]
  const set = (patch: Partial<ModulationEnvelope>) =>
    onChange({ ...value, ...patch })
  return (
    <FieldSet className="border-b px-2.5 py-2">
      <FieldLegend>{title}</FieldLegend>
      <FieldGroup>
        <Enabled
          label={`Enable ${target} envelope`}
          checked={value.enabled}
          onChange={(enabled) => set({ enabled })}
        />
        <FieldGroup className="grid grid-cols-2 justify-items-center gap-2 @min-[22rem]:grid-cols-5">
          <Field>
            <Knob
              label="Attack"
              aria-label={`${title} attack`}
              value={value.attackMs}
              min={0}
              max={10_000}
              defaultValue={2}
              scale={TIME_SCALE}
              showValue
              {...msUnit}
              onValueChange={(attackMs) => set({ attackMs })}
              {...gestures}
            />
          </Field>
          <Field>
            <Knob
              label="Decay"
              aria-label={`${title} decay`}
              value={value.decayMs}
              min={1}
              max={10_000}
              defaultValue={200}
              scale={TIME_SCALE}
              showValue
              {...msUnit}
              onValueChange={(decayMs) => set({ decayMs })}
              {...gestures}
            />
          </Field>
          <Field>
            <Knob
              label="Sustain"
              aria-label={`${title} sustain`}
              value={value.sustain}
              min={0}
              max={1}
              defaultValue={0.8}
              showValue
              {...percentUnit}
              onValueChange={(sustain) => set({ sustain })}
              {...gestures}
            />
          </Field>
          <Field>
            <Knob
              label="Release"
              aria-label={`${title} release`}
              value={value.releaseMs}
              min={1}
              max={10_000}
              defaultValue={150}
              scale={TIME_SCALE}
              showValue
              {...msUnit}
              onValueChange={(releaseMs) => set({ releaseMs })}
              {...gestures}
            />
          </Field>
          <Field>
            <Knob
              label="Depth"
              aria-label={`${title} depth`}
              value={value.depth}
              min={-maxDepth}
              max={maxDepth}
              defaultValue={0}
              bipolar
              showValue
              format={(depth) =>
                `${depth.toFixed(target === "pitch" ? 0 : 2)} ${depthUnit}`
              }
              onValueChange={(depth) => set({ depth })}
              {...gestures}
            />
          </Field>
        </FieldGroup>
      </FieldGroup>
    </FieldSet>
  )
}

function TargetLfoControls({
  target,
  value,
  onChange,
}: {
  target: TargetLfoName
  value: TargetLfo
  onChange(value: TargetLfo): void
}) {
  const gestures = useContext(GestureContext)
  const title = `${target[0].toUpperCase()}${target.slice(1)} LFO`
  const set = (patch: Partial<TargetLfo>) => onChange({ ...value, ...patch })
  return (
    <FieldSet className="border-b px-2.5 py-2">
      <FieldLegend>{title}</FieldLegend>
      <FieldGroup>
        <Enabled
          label={`Enable ${target} LFO`}
          checked={value.enabled}
          onChange={(enabled) => set({ enabled })}
        />
        <Choices
          label={`${title} waveform`}
          value={value.shape}
          options={SHAPES}
          onChange={(shape) => set({ shape })}
        />
        <FieldGroup className="grid grid-cols-2 justify-items-center gap-2">
          <Field>
            <Knob
              label="Rate"
              aria-label={`${title} rate`}
              value={value.rateHz}
              min={0.01}
              max={30}
              defaultValue={5}
              scale="log"
              showValue
              format={(rate) => `${rate.toFixed(2)} Hz`}
              onValueChange={(rateHz) => set({ rateHz })}
              {...gestures}
            />
          </Field>
          <Field>
            <Knob
              label="Depth"
              aria-label={`${title} depth`}
              value={value.depth}
              min={-1}
              max={1}
              defaultValue={0}
              bipolar
              showValue
              {...percentUnit}
              onValueChange={(depth) => set({ depth })}
              {...gestures}
            />
          </Field>
          <Field>
            <Knob
              label="Delay"
              aria-label={`${title} delay`}
              value={value.delayMs}
              min={0}
              max={20_000}
              defaultValue={0}
              showValue
              format={(delay) => `${Math.round(delay)} ms`}
              onValueChange={(delayMs) => set({ delayMs })}
              {...gestures}
            />
          </Field>
        </FieldGroup>
        <Enabled
          label={`Restart ${target} LFO on each note`}
          checked={value.retrigger}
          onChange={(retrigger) => set({ retrigger })}
        />
        <FieldDescription>
          Full depth: {LFO_FULL_DEPTH[target]}.
        </FieldDescription>
      </FieldGroup>
    </FieldSet>
  )
}

export interface ChannelVoicePanelProps {
  /**
   * The saved settings. They are shown whenever no edit of this panel is on
   * its way, so an undo or a load replaces what the panel shows.
   */
  initialSettings?: ChannelVoiceSettings
  /**
   * Called with the complete settings after every edit. Return the promise
   * of the save to let the panel follow the saved settings once it settles;
   * a handler that returns nothing leaves the panel on its own draft.
   */
  onChange(settings: ChannelVoiceSettings): void | Promise<unknown>
  /** A knob drag starts and ends, to group its edits into one undo step. */
  onGestureStart?: () => void
  onGestureEnd?: () => void
  /**
   * An instrument channel has no sampler voice: the channel filter, the
   * resonance target and the volume LFO do not apply to it and are hidden.
   */
  kind?: "sampler" | "instrument"
}

/** Local editor: the parent owns persistence and engine application. */
export function ChannelVoicePanel(props: ChannelVoicePanelProps) {
  const { onGestureStart, onGestureEnd } = props
  const gestures = useMemo(
    () => ({ onGestureStart, onGestureEnd }),
    [onGestureStart, onGestureEnd]
  )
  return (
    <GestureContext.Provider value={gestures}>
      <VoiceEditor {...props} />
    </GestureContext.Provider>
  )
}

function VoiceEditor({
  initialSettings,
  onChange,
  kind = "sampler",
}: ChannelVoicePanelProps) {
  const gestures = useContext(GestureContext)
  const stored = useMemo(
    () =>
      sanitizeChannelVoiceSettings(
        initialSettings ?? createDefaultChannelVoiceSettings()
      ),
    [initialSettings]
  )
  const [local, setLocal] = useState<ChannelVoiceSettings | null>(null)
  const settings = local ?? stored
  // Keep rapid edits coherent before React renders; onChange stays outside a
  // state updater so Strict Mode cannot duplicate external side effects.
  // Read and written in handlers only.
  const draft = useRef<ChannelVoiceSettings | null>(null)
  function update(
    change: (current: ChannelVoiceSettings) => ChannelVoiceSettings
  ) {
    const next = sanitizeChannelVoiceSettings(change(draft.current ?? stored))
    draft.current = next
    setLocal(next)
    const saved: unknown = onChange(next)
    if (saved instanceof Promise) {
      // The save has landed in the project, or was refused: either way the
      // saved settings are the truth again unless a newer edit is under way.
      const settle = () => {
        if (draft.current === next) {
          draft.current = null
          setLocal(null)
        }
      }
      void saved.then(settle, settle)
    }
  }
  const sampler = kind === "sampler"
  const arp = settings.arpeggiator
  const echo = settings.echo
  const poly = settings.polyphony
  const lfo = settings.envelopes.lfo
  const filter = voiceFilterOf(settings)
  const lfos = targetLfosOf(settings)
  const setArp = (patch: Partial<typeof arp>) =>
    update((current) => ({
      ...current,
      arpeggiator: { ...current.arpeggiator, ...patch },
    }))
  const setEcho = (patch: Partial<typeof echo>) =>
    update((current) => ({ ...current, echo: { ...current.echo, ...patch } }))
  const setPoly = (patch: Partial<typeof poly>) =>
    update((current) => ({
      ...current,
      polyphony: { ...current.polyphony, ...patch },
    }))
  const setLfo = (patch: Partial<typeof lfo>) =>
    update((current) => ({
      ...current,
      envelopes: {
        ...current.envelopes,
        lfo: { ...current.envelopes.lfo, ...patch },
      },
    }))
  const setFilter = (patch: Partial<VoiceFilter>) =>
    update((current) => ({
      ...current,
      envelopes: {
        ...current.envelopes,
        filterSettings: { ...voiceFilterOf(current), ...patch },
      },
    }))
  const setEnvelope = (target: EnvelopeTarget, value: ModulationEnvelope) =>
    update((current) => ({
      ...current,
      envelopes: { ...current.envelopes, [target]: value },
    }))
  const setTargetLfo = (target: TargetLfoName, value: TargetLfo) =>
    update((current) => ({
      ...current,
      envelopes: {
        ...current.envelopes,
        lfos: { ...targetLfosOf(current), [target]: value },
      },
    }))
  const envelopeTargets: readonly EnvelopeTarget[] = sampler
    ? ["filter", "resonance", "pitch", "pan"]
    : ["filter", "pitch", "pan"]
  const lfoTargets = LFO_TARGETS.filter(
    (target) => sampler || (target !== "volume" && target !== "resonance")
  )

  return (
    <div
      aria-label="Channel voice settings"
      role="group"
      className="@container flex min-w-0 flex-col text-xs"
    >
      <FieldSet className="border-b px-2.5 py-2">
        <FieldLegend>Arpeggiator</FieldLegend>
        <FieldGroup>
          <Choices
            label="Arpeggiator mode"
            value={arp.mode}
            options={[
              { value: "off", label: "Off" },
              { value: "up", label: "Up" },
              { value: "down", label: "Down" },
              { value: "upDown", label: "Up-down" },
              { value: "asPlayed", label: "As played" },
            ]}
            onChange={(mode) => setArp({ mode })}
          />
          <Choices
            label="Arpeggiator rate"
            value={arp.rate}
            options={DIVISIONS}
            onChange={(rate) => setArp({ rate })}
          />
          <FieldGroup className="grid grid-cols-2 justify-items-center gap-2">
            <Field>
              <Knob
                label="Gate"
                aria-label="Arpeggiator gate"
                value={arp.gate}
                min={0}
                max={1}
                defaultValue={0.75}
                showValue
                {...percentUnit}
                onValueChange={(gate) => setArp({ gate })}
                {...gestures}
              />
            </Field>
            <Field>
              <Knob
                label="Octaves"
                aria-label="Arpeggiator octave range"
                value={arp.rangeOctaves}
                min={1}
                max={4}
                step={1}
                defaultValue={1}
                showValue
                onValueChange={(rangeOctaves) => setArp({ rangeOctaves })}
                {...gestures}
              />
            </Field>
          </FieldGroup>
        </FieldGroup>
      </FieldSet>
      <FieldSet className="border-b px-2.5 py-2">
        <FieldLegend>Note echo</FieldLegend>
        <FieldGroup>
          <Enabled
            label="Enable note echo"
            checked={echo.enabled}
            onChange={(enabled) => setEcho({ enabled })}
          />
          <Choices
            label="Echo timing"
            value={echo.time.unit}
            options={[
              { value: "milliseconds", label: "Milliseconds" },
              { value: "division", label: "Tempo" },
            ]}
            onChange={(unit) => {
              if (unit !== echo.time.unit)
                setEcho({
                  time:
                    unit === "milliseconds"
                      ? { unit, ms: 250 }
                      : { unit, division: "eighth" },
                })
            }}
          />
          {echo.time.unit === "division" ? (
            <Choices
              label="Echo note division"
              value={echo.time.division}
              options={DIVISIONS}
              onChange={(division) =>
                setEcho({ time: { unit: "division", division } })
              }
            />
          ) : (
            <Field>
              <Knob
                label="Delay"
                aria-label="Echo delay time"
                value={echo.time.ms}
                min={1}
                max={60_000}
                defaultValue={250}
                scale={TIME_SCALE}
                showValue
                {...msUnit}
                onValueChange={(ms) =>
                  setEcho({ time: { unit: "milliseconds", ms } })
                }
                {...gestures}
              />
            </Field>
          )}
          <FieldGroup className="grid grid-cols-3 justify-items-center gap-2">
            <Field>
              <Knob
                label="Feedback"
                aria-label="Echo feedback"
                value={echo.feedback}
                min={0}
                max={0.95}
                defaultValue={0.5}
                showValue
                {...percentUnit}
                onValueChange={(feedback) => setEcho({ feedback })}
                {...gestures}
              />
            </Field>
            <Field>
              <Knob
                label="Pitch"
                aria-label="Echo pitch offset"
                value={echo.pitchSemitones}
                min={-48}
                max={48}
                step={1}
                defaultValue={0}
                bipolar
                showValue
                format={(pitch) => `${pitch} st`}
                onValueChange={(pitchSemitones) => setEcho({ pitchSemitones })}
                {...gestures}
              />
            </Field>
            <Field>
              <Knob
                label="Repeats"
                aria-label="Echo repeat count"
                value={echo.repeats}
                min={0}
                max={8}
                step={1}
                defaultValue={3}
                showValue
                onValueChange={(repeats) => setEcho({ repeats })}
                {...gestures}
              />
            </Field>
          </FieldGroup>
          <FieldDescription>
            Each repeat adds the pitch offset and fades by feedback. Very quiet
            repeats stop early.
          </FieldDescription>
        </FieldGroup>
      </FieldSet>
      <FieldSet className="border-b px-2.5 py-2">
        <FieldLegend>Polyphony and glide</FieldLegend>
        <FieldGroup>
          <Enabled
            label="Mono legato"
            checked={poly.monoLegato}
            onChange={(monoLegato) => setPoly({ monoLegato })}
          />
          <FieldGroup className="grid grid-cols-2 justify-items-center gap-2">
            <Field>
              <Knob
                label="Voices"
                aria-label="Maximum channel voices"
                value={poly.maxVoices}
                min={1}
                max={32}
                step={1}
                defaultValue={32}
                showValue
                onValueChange={(maxVoices) => setPoly({ maxVoices })}
                {...gestures}
              />
            </Field>
            <Field>
              <Knob
                label="Portamento"
                aria-label="Portamento time"
                value={poly.portamentoMs}
                min={0}
                max={60_000}
                defaultValue={0}
                scale={TIME_SCALE}
                showValue
                {...msUnit}
                onValueChange={(portamentoMs) => setPoly({ portamentoMs })}
                {...gestures}
              />
            </Field>
          </FieldGroup>
          <FieldDescription>
            Mono legato keeps one voice and preserves envelopes between
            overlapping notes. Portamento time covers 63% of the pitch change.
          </FieldDescription>
        </FieldGroup>
      </FieldSet>
      {sampler && (
        <FieldSet className="border-b px-2.5 py-2">
          <FieldLegend>Channel filter</FieldLegend>
          <FieldGroup>
            <Enabled
              label="Enable channel filter"
              checked={filter.enabled}
              onChange={(enabled) => setFilter({ enabled })}
            />
            <Choices
              label="Filter type"
              value={filter.mode}
              options={FILTER_MODES}
              onChange={(mode) => setFilter({ mode })}
            />
            <FieldGroup className="grid grid-cols-2 justify-items-center gap-2">
              <Field>
                <Knob
                  label="Cutoff"
                  aria-label="Channel filter cutoff"
                  value={filter.cutoffHz}
                  min={MIN_VOICE_CUTOFF_HZ}
                  max={MAX_VOICE_CUTOFF_HZ}
                  defaultValue={2000}
                  scale="log"
                  showValue
                  format={(cutoff) => `${Math.round(cutoff)} Hz`}
                  onValueChange={(cutoffHz) => setFilter({ cutoffHz })}
                  {...gestures}
                />
              </Field>
              <Field>
                <Knob
                  label="Resonance"
                  aria-label="Channel filter resonance"
                  value={filter.resonance}
                  min={0}
                  max={1}
                  defaultValue={0}
                  showValue
                  {...percentUnit}
                  onValueChange={(resonance) => setFilter({ resonance })}
                  {...gestures}
                />
              </Field>
            </FieldGroup>
            <FieldDescription>
              Cutoff and resonance are the two modulation targets: the filter
              and resonance envelopes and the cutoff and resonance LFOs move
              them. The volume envelope is the sampler&apos;s Envelope section.
            </FieldDescription>
          </FieldGroup>
        </FieldSet>
      )}
      {envelopeTargets.map((target) => (
        <EnvelopeControls
          key={target}
          target={target}
          value={
            target === "resonance"
              ? resonanceEnvelopeOf(settings)
              : settings.envelopes[target]
          }
          onChange={(value) => setEnvelope(target, value)}
        />
      ))}
      {lfoTargets.map((target) => (
        <TargetLfoControls
          key={target}
          target={target}
          value={lfos[target]}
          onChange={(value) => setTargetLfo(target, value)}
        />
      ))}
      <FieldSet className="border-b px-2.5 py-2">
        <FieldLegend>Voice LFO</FieldLegend>
        <FieldGroup>
          <Enabled
            label="Enable voice LFO"
            checked={lfo.enabled}
            onChange={(enabled) => setLfo({ enabled })}
          />
          <Choices
            label="LFO waveform"
            value={lfo.shape}
            options={SHAPES}
            onChange={(shape) => setLfo({ shape })}
          />
          <Choices
            label="LFO target"
            value={lfo.target}
            options={
              sampler
                ? [
                    { value: "filter", label: "Filter" },
                    { value: "pitch", label: "Pitch" },
                    { value: "pan", label: "Pan" },
                    { value: "volume", label: "Volume" },
                    { value: "resonance", label: "Resonance" },
                  ]
                : [
                    { value: "filter", label: "Filter" },
                    { value: "pitch", label: "Pitch" },
                    { value: "pan", label: "Pan" },
                  ]
            }
            onChange={(target) => setLfo({ target })}
          />
          <FieldGroup className="grid grid-cols-2 justify-items-center gap-2">
            <Field>
              <Knob
                label="Rate"
                aria-label="LFO rate"
                value={lfo.rateHz}
                min={0.01}
                max={30}
                defaultValue={5}
                scale="log"
                showValue
                format={(rate) => `${rate.toFixed(2)} Hz`}
                onValueChange={(rateHz) => setLfo({ rateHz })}
                {...gestures}
              />
            </Field>
            <Field>
              <Knob
                label="Depth"
                aria-label="LFO depth"
                value={lfo.depth}
                min={-1}
                max={1}
                defaultValue={0}
                bipolar
                showValue
                {...percentUnit}
                onValueChange={(depth) => setLfo({ depth })}
                {...gestures}
              />
            </Field>
          </FieldGroup>
          <FieldDescription>
            One extra LFO for any target. Full depth: 8 filter octaves, 2400
            pitch cents, full pan travel, a tremolo down to silence, or the
            whole resonance range. Negative depth reverses the motion.
          </FieldDescription>
        </FieldGroup>
      </FieldSet>
    </div>
  )
}
