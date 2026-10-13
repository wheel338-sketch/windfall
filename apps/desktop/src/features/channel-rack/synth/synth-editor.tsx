import type { ReactNode } from "react"

import type { ChannelId, ParamChoice } from "@/bindings"
import {
  instrumentDescriptor,
  ParamControl,
  ParamEnvelope,
  ParamGroup,
  ParamRow,
  useParamBinding,
  type ParamBinder,
  type ParamControlProps,
  type SetParam,
} from "@/features/params"
import { useParamAutomation } from "@/features/automation/live"
import { useHint } from "@/lib/store/hint"
import { dispatch } from "@/lib/store/project"
import { cn } from "@/lib/utils"

import { describeSynthParam } from "./descriptions"
import type { SynthSettings } from "./presets"
import { isWaveShape, WaveGlyph } from "./wave-glyph"
import { message, plainNumber, translatedText as t } from "@/lib/i18n"

const descriptor = instrumentDescriptor("subtractiveSynth")

/*
 * The panel is laid out for the width it gets, which the user sets by
 * dragging the edge of the channel settings. `@container` on the root makes
 * the breakpoints below follow that width and not the window's:
 *   under 17.5rem  everything in one column, an oscillator per row
 *   from 17.5rem   three oscillator columns
 *   from 30rem     the filter in one row; the two envelopes side by side,
 *                  and so are voice and output
 *   from 34rem     the two LFOs side by side
 */

/**
 * While the oscillators are three narrow columns, their waveform lists drop
 * the arrows at the end, which leaves room for the names: the drawing of
 * the wave already marks them as lists of waves.
 */
const NARROW_SELECT =
  "@min-[17.5rem]:@max-[26rem]:[&_[data-slot=param-select]>svg:last-child]:hidden"

const waveIcon = (choice: ParamChoice): ReactNode =>
  isWaveShape(choice.value) ? <WaveGlyph shape={choice.value} /> : null

type Extra = Partial<
  Pick<
    ParamControlProps,
    "className" | "choiceIcon" | "choiceStyle" | "layout" | "size"
  >
>

/** The control of one setting, with a short label and its status-bar line. */
function control(
  bind: ParamBinder,
  id: string,
  label: string | null,
  extra?: Extra
) {
  return (
    <ParamControl
      key={id}
      {...bind(id)}
      label={label}
      description={describeSynthParam(id)}
      {...extra}
    />
  )
}

function Oscillator({ bind, index }: { bind: ParamBinder; index: number }) {
  const at = `oscillators.${index}`
  const waveform = bind.info(`${at}.waveform`)
  const pulse =
    waveform.choices[bind.value(`${at}.waveform`)]?.value === "pulse"
  // An oscillator at no level is off. Its other controls still work, and
  // step back so the eye goes to the ones that are sounding.
  const off = bind.value(`${at}.level`) === 0
  const quiet = off ? "opacity-45" : undefined

  return (
    <div
      role="group"
      aria-label={message("Oscillator {index}", { index: plainNumber(index + 1) })}
      data-off={off ? "" : undefined}
      className="flex min-w-0 flex-col gap-2 py-2 first:pt-0 last:pb-0 @[17.5rem]:px-2 @[17.5rem]:py-0 @[17.5rem]:first:pl-0 @[17.5rem]:last:pr-0"
    >
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <span className="flex w-14 shrink-0 items-baseline gap-1.5 text-[0.625rem] leading-none">
          <span className="font-medium text-foreground/80">
            Osc {index + 1}
          </span>
          {off && <span className="text-muted-foreground">{t("off")}</span>}
        </span>
        {control(bind, `${at}.waveform`, null, {
          choiceIcon: waveIcon,
          className: cn("flex-[1_1_4.5rem]", NARROW_SELECT, quiet),
        })}
      </div>
      <div
        className={cn(
          "grid justify-items-center gap-x-1 gap-y-2 @[17.5rem]:grid-cols-2",
          pulse ? "grid-cols-5" : "grid-cols-4"
        )}
      >
        {control(bind, `${at}.level`, "Level")}
        {control(bind, `${at}.pan`, "Pan", { className: quiet })}
        {control(bind, `${at}.coarse`, "Coarse", { className: quiet })}
        {control(bind, `${at}.fineCents`, "Fine", { className: quiet })}
        {pulse &&
          control(bind, `${at}.pulseWidth`, "Width", { className: quiet })}
      </div>
    </div>
  )
}

function Oscillators({ bind }: { bind: ParamBinder }) {
  return (
    <ParamGroup title="Oscillators">
      <div className="grid grid-cols-1 divide-y divide-border/70 @[17.5rem]:grid-cols-3 @[17.5rem]:divide-x @[17.5rem]:divide-y-0">
        {[0, 1, 2].map((index) => (
          <Oscillator key={index} bind={bind} index={index} />
        ))}
      </div>
      <div
        role="group"
        aria-label={t("Unison")}
        className="flex items-center gap-2 border-t border-border/70 pt-2"
      >
        <span className="w-12 shrink-0 text-[0.6875rem] font-medium text-foreground/80">
          {t("Unison")}
        </span>
        <ParamRow columns={3} className="max-w-64 flex-1">
          {control(bind, "unisonVoices", "Voices")}
          {control(bind, "unisonDetuneCents", "Detune")}
          {control(bind, "unisonSpread", "Spread")}
        </ParamRow>
      </div>
    </ParamGroup>
  )
}

function Filter({ bind }: { bind: ParamBinder }) {
  return (
    <ParamGroup title="Filter">
      <div className="flex flex-wrap gap-x-2 gap-y-1.5">
        {control(bind, "filter.mode", null, {
          className: "flex-[1_1_11rem]",
        })}
        {control(bind, "filter.slope", null, {
          className: "flex-[1_1_8rem]",
        })}
      </div>
      <div className="grid grid-cols-3 justify-items-center gap-x-1 gap-y-2 @[30rem]:grid-cols-6">
        {control(bind, "filter.cutoffHz", "Cutoff")}
        {control(bind, "filter.resonance", "Resonance")}
        {control(bind, "filter.drive", "Drive")}
        {control(bind, "filter.envelopeOctaves", "Envelope")}
        {control(bind, "filter.keyTracking", "Key track")}
        {control(bind, "filter.velocity", "Velocity")}
      </div>
    </ParamGroup>
  )
}

function Envelope({
  bind,
  title,
  prefix,
  color,
}: {
  bind: ParamBinder
  title: string
  prefix: "ampEnvelope" | "filterEnvelope"
  color: string
}) {
  const hint = useHint(
    message("{title}: drag the nodes for attack, decay and sustain level, and release. Double-click a node to reset it", { title })
  )
  return (
    <ParamGroup title={title}>
      <ParamEnvelope
        bind={bind}
        prefix={prefix}
        aria-label={message("{title} shape", { title })}
        color={color}
        className="h-20"
        {...hint}
      />
      <ParamRow columns={4}>
        {control(bind, `${prefix}.attackMs`, "Attack")}
        {control(bind, `${prefix}.decayMs`, "Decay")}
        {control(bind, `${prefix}.sustain`, "Sustain")}
        {control(bind, `${prefix}.releaseMs`, "Release")}
      </ParamRow>
    </ParamGroup>
  )
}

function Lfo({ bind, index }: { bind: ParamBinder; index: number }) {
  const at = `lfos.${index}`
  return (
    <ParamGroup
      title={`LFO ${index + 1}`}
      aside={control(bind, `${at}.shape`, null, {
        choiceIcon: waveIcon,
        className: "w-28",
      })}
    >
      <ParamRow columns={5}>
        {control(bind, `${at}.rateHz`, "Rate")}
        {control(bind, `${at}.pitchSemitones`, "Pitch")}
        {control(bind, `${at}.cutoffOctaves`, "Cutoff")}
        {control(bind, `${at}.amp`, "Volume")}
        {control(bind, `${at}.pulseWidth`, "Width")}
      </ParamRow>
    </ParamGroup>
  )
}

function Voice({ bind }: { bind: ParamBinder }) {
  const mode = bind.info("voiceMode")
  const poly = mode.choices[bind.value("voiceMode")]?.value === "poly"
  return (
    <ParamGroup title="Voice">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        {control(bind, "voiceMode", null, {
          className: "flex-[1_1_8.5rem]",
        })}
        <ParamRow columns={2} className="flex-[1_1_6rem]">
          {control(bind, "glideMs", "Glide")}
          {/* Only poly mode counts notes. */}
          {control(bind, "polyphony", "Polyphony", {
            className: poly ? undefined : "opacity-45",
          })}
        </ParamRow>
      </div>
    </ParamGroup>
  )
}

function Output({ bind }: { bind: ParamBinder }) {
  return (
    <ParamGroup title="Output">
      <ParamRow columns={3}>
        {control(bind, "ampVelocity", "Velocity")}
        {control(bind, "gain", "Volume")}
        {control(bind, "pan", "Pan")}
      </ParamRow>
    </ParamGroup>
  )
}

type SynthEditorProps = {
  channel: ChannelId
  params: SynthSettings
  /** The channel's color, for the envelopes. */
  color: string
  className?: string
}

/**
 * Every setting of the subtractive synth, in the order the sound is made:
 * oscillators, filter, the envelopes that shape a note, the LFOs that move
 * it, and how notes are played and sent out. Each control edits the
 * project through `setInstrumentParam`, one undo step per drag.
 */
export function SynthEditor({
  channel,
  params,
  color,
  className,
}: SynthEditorProps) {
  const setParam: SetParam = (param, value, gesture) =>
    dispatch({ type: "setInstrumentParam", channel, param, value }, gesture)
  // Every setting can be automated from its own control.
  const automation = useParamAutomation(
    (param) => ({ type: "instrumentParam", channel, param }),
    String(channel)
  )
  const bind = useParamBinding({
    descriptor,
    params,
    setParam,
    ...automation,
  })

  return (
    <div
      data-slot="synth-editor"
      className={cn("@container flex min-w-0 flex-col gap-2", className)}
    >
      <Oscillators bind={bind} />
      <Filter bind={bind} />
      <div className="grid gap-2 @[30rem]:grid-cols-2">
        <Envelope
          bind={bind}
          title={t("Amp envelope")}
          prefix="ampEnvelope"
          color={color}
        />
        <Envelope
          bind={bind}
          title={t("Filter envelope")}
          prefix="filterEnvelope"
          color={color}
        />
      </div>
      <div className="grid gap-2 @[34rem]:grid-cols-2">
        <Lfo bind={bind} index={0} />
        <Lfo bind={bind} index={1} />
      </div>
      <div className="grid gap-2 @[30rem]:grid-cols-2">
        <Voice bind={bind} />
        <Output bind={bind} />
      </div>
    </div>
  )
}
