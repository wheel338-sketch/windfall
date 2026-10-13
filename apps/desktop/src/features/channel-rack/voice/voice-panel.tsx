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
import { translatedText as t, message } from "@/lib/i18n"

/** The targets that have an envelope in this panel. */
type EnvelopeTarget = "filter" | "pitch" | "pan" | "resonance"

type GestureHandlers = {
  onGestureStart?: () => void
  onGestureEnd?: () => void
}
/** Every knob of the panel reports the start and end of a drag through this. */
const GestureContext = createContext<GestureHandlers>({})

const SHAPES: readonly { value: LfoShape; label: string }[] = [
  { value: "sine", get label() { return t("Sine") } },
  { value: "triangle", get label() { return t("Triangle") } },
  { value: "square", get label() { return t("Square") } },
]
const FILTER_MODES: readonly { value: VoiceFilterMode; label: string }[] = [
  { value: "lowPass", get label() { return t("Low-pass") } },
  { value: "highPass", get label() { return t("High-pass") } },
  { value: "bandPass", get label() { return t("Band-pass") } },
  { value: "notch", get label() { return t("Notch") } },
]
const LFO_FULL_DEPTH: Record<TargetLfoName, string> = {
  get volume() { return t("down to silence") },
  get pan() { return t("full pan travel") },
  get pitch() { return t("2400 cents") },
  get cutoff() { return t("8 octaves") },
  get resonance() { return t("the whole resonance range") },
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
  const title = target === "filter" ? t("Filter envelope") : target === "pitch" ? t("Pitch envelope") : target === "pan" ? t("Pan envelope") : t("Resonance envelope")
  const { max: maxDepth, unit: depthUnit } = ENVELOPE_DEPTH[target]
  const set = (patch: Partial<ModulationEnvelope>) =>
    onChange({ ...value, ...patch })
  return (
    <FieldSet className="border-b px-2.5 py-2">
      <FieldLegend>{title}</FieldLegend>
      <FieldGroup>
        <Enabled
          label={message("Enable {target} envelope", { target: t(target) })}
          checked={value.enabled}
          onChange={(enabled) => set({ enabled })}
        />
        <FieldGroup className="grid grid-cols-2 justify-items-center gap-2 @min-[22rem]:grid-cols-5">
          <Field>
            <Knob
              label={t("Attack")}
              aria-label={message("{title} attack", { title })}
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
              label={t("Decay")}
              aria-label={message("{title} decay", { title })}
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
              label={t("Sustain")}
              aria-label={message("{title} sustain", { title })}
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
              label={t("Release")}
              aria-label={message("{title} release", { title })}
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
              label={t("Depth")}
              aria-label={message("{title} depth", { title })}
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

/** A whole title per target, so each language orders the words its own way. */
const LFO_TITLES = {
  volume: "Volume LFO",
  pan: "Pan LFO",
  pitch: "Pitch LFO",
  cutoff: "Cutoff LFO",
  resonance: "Resonance LFO",
} as const satisfies Record<TargetLfoName, string>

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
  const title = t(LFO_TITLES[target])
  const set = (patch: Partial<TargetLfo>) => onChange({ ...value, ...patch })
  return (
    <FieldSet className="border-b px-2.5 py-2">
      <FieldLegend>{title}</FieldLegend>
      <FieldGroup>
        <Enabled
          label={message("Enable {target} LFO", { target: t(target) })}
          checked={value.enabled}
          onChange={(enabled) => set({ enabled })}
        />
        <Choices
          label={message("{title} waveform", { title })}
          value={value.shape}
          options={SHAPES}
          onChange={(shape) => set({ shape })}
        />
        <FieldGroup className="grid grid-cols-2 justify-items-center gap-2">
          <Field>
            <Knob
              label={t("Rate")}
              aria-label={message("{title} rate", { title })}
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
              label={t("Depth")}
              aria-label={message("{title} depth", { title })}
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
              label={t("Delay")}
              aria-label={message("{title} delay", { title })}
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
          label={message("Restart {target} LFO on each note", { target: t(target) })}
          checked={value.retrigger}
          onChange={(retrigger) => set({ retrigger })}
        />
        <FieldDescription>
          {t("Full depth:")}{" "}{LFO_FULL_DEPTH[target]}.
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
      aria-label={t("Channel voice settings")}
      role="group"
      className="@container flex min-w-0 flex-col text-xs"
    >
      <FieldSet className="border-b px-2.5 py-2">
        <FieldLegend>{t("Arpeggiator")}</FieldLegend>
        <FieldGroup>
          <Choices
            label={t("Arpeggiator mode")}
            value={arp.mode}
            options={[
              { value: "off", label: t("Off") },
              { value: "up", label: t("Up") },
              { value: "down", label: t("Down") },
              { value: "upDown", label: t("Up-down") },
              { value: "asPlayed", label: t("As played") },
            ]}
            onChange={(mode) => setArp({ mode })}
          />
          <Choices
            label={t("Arpeggiator rate")}
            value={arp.rate}
            options={DIVISIONS}
            onChange={(rate) => setArp({ rate })}
          />
          <FieldGroup className="grid grid-cols-2 justify-items-center gap-2">
            <Field>
              <Knob
                label={t("Gate")}
                aria-label={t("Arpeggiator gate")}
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
                label={t("Octaves")}
                aria-label={t("Arpeggiator octave range")}
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
        <FieldLegend>{t("Note echo")}</FieldLegend>
        <FieldGroup>
          <Enabled
            label={t("Enable note echo")}
            checked={echo.enabled}
            onChange={(enabled) => setEcho({ enabled })}
          />
          <Choices
            label={t("Echo timing")}
            value={echo.time.unit}
            options={[
              { value: "milliseconds", label: t("Milliseconds") },
              { value: "division", label: t("Tempo") },
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
              label={t("Echo note division")}
              value={echo.time.division}
              options={DIVISIONS}
              onChange={(division) =>
                setEcho({ time: { unit: "division", division } })
              }
            />
          ) : (
            <Field>
              <Knob
                label={t("Delay")}
                aria-label={t("Echo delay time")}
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
                label={t("Feedback")}
                aria-label={t("Echo feedback")}
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
                label={t("Pitch")}
                aria-label={t("Echo pitch offset")}
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
                label={t("Repeats")}
                aria-label={t("Echo repeat count")}
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
            {t("Each repeat adds the pitch offset and fades by feedback. Very quiet repeats stop early.")}
          </FieldDescription>
        </FieldGroup>
      </FieldSet>
      <FieldSet className="border-b px-2.5 py-2">
        <FieldLegend>{t("Polyphony and glide")}</FieldLegend>
        <FieldGroup>
          <Enabled
            label={t("Mono legato")}
            checked={poly.monoLegato}
            onChange={(monoLegato) => setPoly({ monoLegato })}
          />
          <FieldGroup className="grid grid-cols-2 justify-items-center gap-2">
            <Field>
              <Knob
                label={t("Voices")}
                aria-label={t("Maximum channel voices")}
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
                label={t("Portamento")}
                aria-label={t("Portamento time")}
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
            {t("Mono legato keeps one voice and preserves envelopes between overlapping notes. Portamento time covers 63% of the pitch change.")}
          </FieldDescription>
        </FieldGroup>
      </FieldSet>
      {sampler && (
        <FieldSet className="border-b px-2.5 py-2">
          <FieldLegend>{t("Channel filter")}</FieldLegend>
          <FieldGroup>
            <Enabled
              label={t("Enable channel filter")}
              checked={filter.enabled}
              onChange={(enabled) => setFilter({ enabled })}
            />
            <Choices
              label={t("Filter type")}
              value={filter.mode}
              options={FILTER_MODES}
              onChange={(mode) => setFilter({ mode })}
            />
            <FieldGroup className="grid grid-cols-2 justify-items-center gap-2">
              <Field>
                <Knob
                  label={t("Cutoff")}
                  aria-label={t("Channel filter cutoff")}
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
                  label={t("Resonance")}
                  aria-label={t("Channel filter resonance")}
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
              {t("Cutoff and resonance are the two modulation targets: the filter and resonance envelopes and the cutoff and resonance LFOs move them. The volume envelope is the sampler's Envelope section.")}
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
        <FieldLegend>{t("Voice LFO")}</FieldLegend>
        <FieldGroup>
          <Enabled
            label={t("Enable voice LFO")}
            checked={lfo.enabled}
            onChange={(enabled) => setLfo({ enabled })}
          />
          <Choices
            label={t("LFO waveform")}
            value={lfo.shape}
            options={SHAPES}
            onChange={(shape) => setLfo({ shape })}
          />
          <Choices
            label={t("LFO target")}
            value={lfo.target}
            options={
              sampler
                ? [
                    { value: "filter", label: t("Filter") },
                    { value: "pitch", label: t("Pitch") },
                    { value: "pan", label: t("Pan") },
                    { value: "volume", label: t("Volume") },
                    { value: "resonance", label: t("Resonance") },
                  ]
                : [
                    { value: "filter", label: t("Filter") },
                    { value: "pitch", label: t("Pitch") },
                    { value: "pan", label: t("Pan") },
                  ]
            }
            onChange={(target) => setLfo({ target })}
          />
          <FieldGroup className="grid grid-cols-2 justify-items-center gap-2">
            <Field>
              <Knob
                label={t("Rate")}
                aria-label={t("LFO rate")}
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
                label={t("Depth")}
                aria-label={t("LFO depth")}
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
            {t("One extra LFO for any target. Full depth: 8 filter octaves, 2400 pitch cents, full pan travel, a tremolo down to silence, or the whole resonance range. Negative depth reverses the motion.")}
          </FieldDescription>
        </FieldGroup>
      </FieldSet>
    </div>
  )
}
