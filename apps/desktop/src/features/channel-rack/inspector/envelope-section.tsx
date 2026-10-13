import { message as i18nMessage } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import type { Channel, ChannelId, Envelope } from "@/bindings"
import {
  DEFAULT_ENVELOPE_LIMITS,
  EnvelopeEditor,
  Knob,
  msUnit,
  percentUnit,
  powerScale,
} from "@/components/audio"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import type { SamplerChannel } from "@/lib/channel-source"
import { useHint } from "@/lib/store/hint"
import { dispatch } from "@/lib/store/project"
import { onProjectReplaced } from "@/lib/store/replaced"
import { clamp, colorToCss } from "@/lib/units"

import { useGestureValue } from "../use-gesture-value"
import { nextEnvelopePreset } from "./envelope-preset-step"
import { ENVELOPE_PRESETS, nextEnvelope } from "./envelope-presets"
import { nextEnvelopeScale } from "./envelope-scale"
import { Section } from "./parts"
import { nextSustainScale } from "./sustain-scale"

/** What a sampler gets when its envelope is first turned on. */
export const DEFAULT_ENVELOPE: Envelope = {
  attackMs: 1,
  decayMs: 200,
  sustain: 1,
  releaseMs: 50,
}

const TIME_SCALE = powerScale(3)
const { maxAttackMs, maxDecayMs, maxReleaseMs } = DEFAULT_ENVELOPE_LIMITS

// Turning the envelope off and on again brings back the shape it had.
const remembered = new Map<ChannelId, Envelope>()
// Channel ids start over in every project.
onProjectReplaced(() => remembered.clear())

function withinLimits(envelope: Envelope): Envelope {
  return {
    attackMs: clamp(envelope.attackMs, 0, maxAttackMs),
    decayMs: clamp(envelope.decayMs, 0, maxDecayMs),
    sustain: clamp(envelope.sustain, 0, 1),
    releaseMs: clamp(envelope.releaseMs, 0, maxReleaseMs),
  }
}

function EnvelopeControls({
  channel,
  envelope: stored,
}: {
  channel: Channel
  envelope: Envelope
}) {
  useLanguageStore((state) => state.language)
  const { id } = channel
  const envelope = useGestureValue(stored, (value, send) =>
    send({ type: "setSamplerEnvelope", id, envelope: withinLimits(value) })
  )
  const value = envelope.value
  const set = (patch: Partial<Envelope>) =>
    envelope.onValueChange({ ...value, ...patch })
  const gesture = {
    onGestureStart: envelope.onGestureStart,
    onGestureEnd: envelope.onGestureEnd,
  }

  const editorHint = useHint(
    t("Drag the nodes: attack, then decay and sustain level, then release. Double-click a node to reset it")
  )
  const attackHint = useHint(t("Attack: how long the sound takes to fade in"))
  const decayHint = useHint(
    t("Decay: how long it takes to fall to the sustain level")
  )
  const sustainHint = useHint(
    t("Sustain: the level held for as long as the note lasts")
  )
  const releaseHint = useHint(
    t("Release: how long the sound rings on after the note ends")
  )

  return (
    <>
      <EnvelopeEditor
        aria-label={t("Envelope shape")}
        {...value}
        defaults={DEFAULT_ENVELOPE}
        // The sampler's envelope: a straight attack, exponential falls.
        curve="exponential"
        color={colorToCss(channel.color)}
        onChange={set}
        className="h-28"
        {...gesture}
        {...editorHint}
      />
      <div className="grid grid-cols-4 justify-items-center gap-1">
        <Knob
          label={t("Attack")}
          showValue
          min={0}
          max={maxAttackMs}
          scale={TIME_SCALE}
          defaultValue={DEFAULT_ENVELOPE.attackMs}
          value={value.attackMs}
          onValueChange={(attackMs) => set({ attackMs })}
          {...msUnit}
          {...gesture}
          {...attackHint}
        />
        <Knob
          label={t("Decay")}
          showValue
          min={0}
          max={maxDecayMs}
          scale={TIME_SCALE}
          defaultValue={DEFAULT_ENVELOPE.decayMs}
          value={value.decayMs}
          onValueChange={(decayMs) => set({ decayMs })}
          {...msUnit}
          {...gesture}
          {...decayHint}
        />
        <Knob
          label={t("Sustain")}
          showValue
          min={0}
          max={1}
          defaultValue={DEFAULT_ENVELOPE.sustain}
          value={value.sustain}
          onValueChange={(sustain) => set({ sustain })}
          {...percentUnit}
          {...gesture}
          {...sustainHint}
        />
        <Knob
          label={t("Release")}
          showValue
          min={0}
          max={maxReleaseMs}
          scale={TIME_SCALE}
          defaultValue={DEFAULT_ENVELOPE.releaseMs}
          value={value.releaseMs}
          onValueChange={(releaseMs) => set({ releaseMs })}
          {...msUnit}
          {...gesture}
          {...releaseHint}
        />
      </div>
      <div className="grid grid-cols-4 gap-1">
        {ENVELOPE_PRESETS.map(({ label, envelope: preset }) => (
          <Button
            key={label}
            variant="outline"
            size="sm"
            disabled={nextEnvelope(value, preset) === null}
            onClick={() => {
              const next = nextEnvelope(value, preset)
              if (next !== null) {
                void dispatch({
                  type: "setSamplerEnvelope",
                  id,
                  envelope: next,
                })
              }
            }}
          >
            {t(label)}
          </Button>
        ))}
      </div>
      <div className="flex gap-1">
        {(
          [
            [t("Previous"), "previous"],
            [t("Next"), "next"],
          ] as const
        ).map(([label, direction]) => (
          <Button
            key={direction}
            variant="outline"
            size="sm"
            aria-label={i18nMessage("Choose the {value0} envelope preset", { value0: t(direction) })}
            disabled={nextEnvelopePreset(value, direction) === null}
            onClick={() => {
              const next = nextEnvelopePreset(value, direction)
              if (next !== null) {
                void dispatch({
                  type: "setSamplerEnvelope",
                  id,
                  envelope: next,
                })
              }
            }}
          >
            {t(label)}
          </Button>
        ))}
      </div>
      <div className="flex gap-1">
        {(
          [
            [t("Half"), 0.5],
            [t("Double"), 2],
          ] as const
        ).map(([label, factor]) => (
          <Button
            key={label}
            variant="outline"
            size="sm"
            disabled={nextEnvelopeScale(value, factor) === null}
            onClick={() => {
              const next = nextEnvelopeScale(value, factor)
              if (next !== null) {
                void dispatch({
                  type: "setSamplerEnvelope",
                  id,
                  envelope: next,
                })
              }
            }}
          >
            {t(label)}
          </Button>
        ))}
      </div>
      <div className="flex gap-1">
        {(
          [
            [t("Halve sustain"), 0.5],
            [t("Double sustain"), 2],
          ] as const
        ).map(([label, factor]) => (
          <Button
            key={label}
            variant="outline"
            size="sm"
            disabled={nextSustainScale(value, factor) === null}
            onClick={() => {
              const next = nextSustainScale(value, factor)
              if (next !== null) {
                void dispatch({
                  type: "setSamplerEnvelope",
                  id,
                  envelope: next,
                })
              }
            }}
          >
            {t(label)}
          </Button>
        ))}
      </div>
    </>
  )
}

/** The sampler's volume envelope: on or off, and its shape when on. */
export function EnvelopeSection({ channel }: { channel: SamplerChannel }) {
  useLanguageStore((state) => state.language)
  const { id } = channel
  const envelope = channel.source.envelope
  const looping = (channel.source.loopMode ?? "off") !== "off"
  const hint = useHint(
    looping
      ? t("Envelope: shape held loops and their release. When off, looped notes use a short release")
      : t("Envelope: when on, the note's length shapes the sound. When off, every hit plays the sample to its end")
  )

  function setOn(on: boolean) {
    if (on) {
      void dispatch({
        type: "setSamplerEnvelope",
        id,
        envelope: remembered.get(id) ?? DEFAULT_ENVELOPE,
      })
    } else {
      if (envelope) remembered.set(id, envelope)
      void dispatch({ type: "setSamplerEnvelope", id })
    }
  }

  return (
    <Section
      title={t("Envelope")}
      aside={
        <span className="flex items-center gap-1.5" {...hint}>
          {/* Not a label: the switch is named for what it does, not its state. */}
          <span aria-hidden className="text-muted-foreground">
            {envelope ? t("On") : t("Off")}
          </span>
          <Switch
            size="sm"
            aria-label={t("Volume envelope")}
            checked={envelope !== null}
            onCheckedChange={(on) => setOn(on)}
          />
        </span>
      }
    >
      {envelope ? (
        <EnvelopeControls channel={channel} envelope={envelope} />
      ) : (
        <p className="text-muted-foreground">
          {looping
            ? t("Off. Looped notes hold until note-off, then release over 4 ms. Turn it on to shape the loop and its release.")
            : t("Off. Every hit plays the sample to its end, which suits drums. Turn it on to shape held notes.")}
        </p>
      )}
    </Section>
  )
}
