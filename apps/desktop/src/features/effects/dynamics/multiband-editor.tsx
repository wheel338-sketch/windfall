import type {
  MultibandCompressorParams,
  MultibandMaximizerParams,
} from "@/bindings"
import {
  formatParamDisplay,
  ParamControl,
  ParamGroup,
  ParamRow,
  type ParamBinder,
} from "@/features/params"

import type { EditorProps } from "../editor-props"
import { GainReductionMeter } from "../gain-reduction"
import { formatLatency } from "../limiter/latency"
import { useSampleRate } from "../sample-rate"
import { BandDisplay, type BandSetting } from "./band-display"
import { BANDS, maximizerLatencyFrames, type BandKey } from "./curves"
import { message, translatedText as t } from "@/lib/i18n"

const BAND_HINTS: Record<BandKey, string> = {
  low: "the lows",
  mid: "the middle",
  high: "the highs",
}

function BandGroup({
  bind,
  band,
  title,
  ceiling,
}: {
  bind: ParamBinder
  band: BandKey
  title: string
  ceiling: boolean
}) {
  const what = BAND_HINTS[band]
  return (
    <ParamGroup title={title} data-band={band}>
      <ParamRow columns={ceiling ? 3 : 5}>
        <ParamControl
          {...bind(`${band}.thresholdDb`)}
          label="Threshold"
          description={message("The level above which {what} are turned down, measured in this band alone", { what })}
        />
        <ParamControl
          {...bind(`${band}.ratio`)}
          label="Ratio"
          description={message("How hard {what} are turned down over the threshold", { what })}
        />
        <ParamControl
          {...bind(`${band}.attackMs`)}
          label="Attack"
          description={message("How quickly the gain of {what} drops", { what })}
        />
        <ParamControl
          {...bind(`${band}.releaseMs`)}
          label="Release"
          description={message("How quickly the gain of {what} recovers", { what })}
        />
        <ParamControl
          {...bind(`${band}.makeupDb`)}
          label="Makeup"
          description={message("Gain added to {what} after compression", { what })}
        />
        {ceiling && (
          <ParamControl
            {...bind(`${band}CeilingDb`)}
            label="Ceiling"
            description={message("No sample of {what} gets past this level before the bands are added up", { what })}
          />
        )}
      </ParamRow>
    </ParamGroup>
  )
}

/** What the display says for assistive technology. */
function splitLabel(bind: ParamBinder): string {
  const low = formatParamDisplay(
    bind.info("lowCrossoverHz"),
    bind.value("lowCrossoverHz")
  )
  const high = formatParamDisplay(
    bind.info("highCrossoverHz"),
    bind.value("highCrossoverHz")
  )
  return message("Three bands split at {low} and {high}", { low, high })
}

function Editor({
  effect,
  bind,
  name,
  maximizer,
}: {
  effect: EditorProps<object>["effect"]
  bind: ParamBinder
  name: string
  maximizer: boolean
}) {
  const sampleRate = useSampleRate()
  const setting = (band: BandKey): BandSetting => ({
    thresholdDb: bind.value(`${band}.thresholdDb`),
    ratio: bind.value(`${band}.ratio`),
    ceilingDb: maximizer ? bind.value(`${band}CeilingDb`) : undefined,
  })
  return (
    <div
      data-slot="multiband-editor"
      role="group"
      aria-label={message("{name} settings", { name })}
      className="flex max-w-[44rem] flex-col gap-2"
    >
      <div className="flex items-stretch gap-2.5">
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <BandDisplay
            lowCrossoverHz={bind.value("lowCrossoverHz")}
            highCrossoverHz={bind.value("highCrossoverHz")}
            sampleRate={sampleRate}
            bands={[setting("low"), setting("mid"), setting("high")]}
            label={splitLabel(bind)}
          />
          <ParamRow columns={maximizer ? 2 : 3}>
            <ParamControl
              {...bind("lowCrossoverHz")}
              label="Low split"
              description="Where the lows end and the middle begins"
            />
            <ParamControl
              {...bind("highCrossoverHz")}
              label="High split"
              description="Where the middle ends and the highs begin. It stays at least a quarter above the low split"
            />
            {!maximizer && (
              <ParamControl
                {...bind("sidechain")}
                description="Each band listens to its share of the mixer's key input instead of to its own signal"
              />
            )}
          </ParamRow>
        </div>
        <GainReductionMeter effect={effect} label={name} className="h-auto" />
      </div>
      <div className="grid grid-cols-1 gap-2 @min-[34rem]/editor:grid-cols-3">
        {BANDS.map(({ key, title }) => (
          <BandGroup
            key={key}
            bind={bind}
            band={key}
            title={title}
            ceiling={maximizer}
          />
        ))}
      </div>
      {maximizer && (
        <ParamGroup title="Output">
          <ParamRow columns={4}>
            <ParamControl
              {...bind("inputGainDb")}
              label="Input"
              description="Turn it up to push the mix into the bands and make it louder"
            />
            <ParamControl
              {...bind("ceilingDb")}
              description="No output sample gets past this level once the bands are added up"
            />
            <ParamControl
              {...bind("limiterReleaseMs")}
              label="Release"
              description="How quickly the limiter on the sum recovers after a peak"
            />
            <ParamControl
              {...bind("sidechain")}
              description="Each band's compressor listens to its share of the mixer's key input. The limiters always watch the signal itself"
            />
          </ParamRow>
          <p
            data-slot="maximizer-latency"
            className="text-[10px] leading-snug text-muted-foreground"
          >
            {t("The band limiters and the limiter on the sum look ahead, which delays this track by")}{" "}
            <span className="font-readout text-foreground/85">
              {formatLatency(maximizerLatencyFrames(sampleRate), sampleRate)}
            </span>
            {t(". The engine delays every other track to match.")}
          </p>
        </ParamGroup>
      )}
    </div>
  )
}

/**
 * The three-band compressor: the bands across the spectrum, the two
 * splits, and a compressor for each band that hears only its own band.
 */
export function MultibandCompressorEditor({
  effect,
  bind,
}: EditorProps<MultibandCompressorParams>) {
  return (
    <Editor
      effect={effect}
      bind={bind}
      name="Multiband Compressor"
      maximizer={false}
    />
  )
}

/**
 * The multiband maximizer: the compressor's bands, each with a limiter of
 * its own, then a limiter on their sum.
 */
export function MultibandMaximizerEditor({
  effect,
  bind,
}: EditorProps<MultibandMaximizerParams>) {
  return (
    <Editor effect={effect} bind={bind} name="Multiband Maximizer" maximizer />
  )
}
