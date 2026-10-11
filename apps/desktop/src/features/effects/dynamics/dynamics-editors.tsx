import type {
  BassHarmonicsParams,
  MasterLimiterParams,
  OneKnobParams,
  StageStackParams,
  TransientShaperParams,
} from "@/bindings"
import {
  formatParam,
  ParamControl,
  ParamGroup,
  ParamRow,
} from "@/features/params"

import type { EditorProps } from "../editor-props"
import { GainReductionMeter } from "../gain-reduction"
import { formatLatency } from "../limiter/latency"
import { useSampleRate } from "../sample-rate"
import {
  HARMONICS_LATENCY_FRAMES,
  masterLimiterLatencyFrames,
  maximizerLatencyFrames,
  oneKnobRecipe,
  TRANSIENT_RANGE_DB,
} from "./curves"
import { TransientDisplay } from "./transient-display"

const NOTE = "text-[10px] leading-snug text-muted-foreground"
const READOUT = "font-readout text-foreground/85"

/**
 * The one-knob enhancer. There is one control on purpose: the knob moves
 * every setting of a multiband maximizer together, and the line under it
 * says where that has put them.
 */
export function OneKnobEditor({ effect, bind }: EditorProps<OneKnobParams>) {
  const sampleRate = useSampleRate()
  const recipe = oneKnobRecipe(bind.value("amount"))
  return (
    <div
      data-slot="one-knob-editor"
      role="group"
      aria-label="One Knob settings"
      className="flex max-w-[26rem] items-center gap-4"
    >
      <ParamControl
        {...bind("amount")}
        size="lg"
        description="How much louder, denser and brighter. At zero the sound passes as it came in"
      />
      <GainReductionMeter effect={effect} label="One Knob" className="h-24" />
      <div className="flex min-w-0 flex-col gap-1">
        <p data-slot="one-knob-recipe" className={NOTE}>
          Drive{" "}
          <span className={READOUT}>+{recipe.inputGainDb.toFixed(1)} dB</span>{" "}
          into three bands at{" "}
          <span className={READOUT}>
            {recipe.low.ratio.toFixed(1)}:1, {recipe.mid.ratio.toFixed(1)}:1
            and {recipe.high.ratio.toFixed(1)}:1
          </span>
          , with a ceiling of{" "}
          <span className={READOUT}>{recipe.ceilingDb.toFixed(1)} dB</span>.
        </p>
        <p data-slot="one-knob-latency" className={NOTE}>
          Delays this track by{" "}
          <span className={READOUT}>
            {formatLatency(maximizerLatencyFrames(sampleRate), sampleRate)}
          </span>
          . For the bands themselves, use the Multiband Maximizer.
        </p>
      </div>
    </div>
  )
}

/**
 * The transient shaper: the front of each hit and what follows it, turned
 * up or down apart, with a picture of what that does to a hit and a switch
 * for hearing either part alone.
 */
export function TransientShaperEditor({
  bind,
}: EditorProps<TransientShaperParams>) {
  const attack = bind.value("attack")
  const sustain = bind.value("sustain")
  const listen = bind.value("listen")
  const db = (amount: number) => {
    const value = amount * TRANSIENT_RANGE_DB
    return `${value < 0 ? "−" : "+"}${Math.abs(value).toFixed(1)} dB`
  }
  return (
    <div
      data-slot="transient-editor"
      role="group"
      aria-label="Transient Shaper settings"
      className="flex max-w-[30rem] flex-col gap-2"
    >
      <TransientDisplay attack={attack} sustain={sustain} listen={listen} />
      <ParamRow columns={3}>
        <ParamControl
          {...bind("attack")}
          description="Boosts or softens the first milliseconds of each hit, by up to 18 dB"
        />
        <ParamControl
          {...bind("sustain")}
          description="Boosts or softens what follows the attack: the body and the release, by up to 18 dB"
        />
        <ParamControl
          {...bind("sensitivity")}
          description="How readily a rise in level counts as an attack"
        />
      </ParamRow>
      <ParamControl
        {...bind("listen")}
        layout="inline"
        description="Lets out the whole sound, or only its attack or its sustain. The two parts add up to the whole, so two shapers on two tracks split a sound in two"
      />
      <p data-slot="transient-amounts" className={NOTE}>
        Attack <span className={READOUT}>{db(attack)}</span>, sustain{" "}
        <span className={READOUT}>{db(sustain)}</span>.
      </p>
    </div>
  )
}

/**
 * Bass harmonics: where the bass ends, how hard it is driven, and how much
 * of what that generates is added, with where the new harmonics land.
 */
export function BassHarmonicsEditor({
  bind,
}: EditorProps<BassHarmonicsParams>) {
  const sampleRate = useSampleRate()
  const info = bind.info("cutoffHz")
  const cutoff = bind.value("cutoffHz")
  return (
    <div
      data-slot="bass-harmonics-editor"
      role="group"
      aria-label="Bass Harmonics settings"
      className="flex max-w-[26rem] flex-col gap-2"
    >
      <ParamRow columns={3}>
        <ParamControl
          {...bind("cutoffHz")}
          description="Everything below this is treated as bass and has harmonics made from it"
        />
        <ParamControl
          {...bind("drive")}
          description="How hard the bass is pushed into the harmonic generator. More drive gives denser harmonics"
        />
        <ParamControl
          {...bind("amount")}
          description="How much of the generated harmonics is added. The bass itself is left as it is"
        />
      </ParamRow>
      <p data-slot="bass-harmonics-range" className={NOTE}>
        Notes below{" "}
        <span className={READOUT}>{formatParam(info, cutoff)}</span> gain an
        octave above them, up to{" "}
        <span className={READOUT}>
          {formatParam({ ...info, max: info.max * 2 }, cutoff * 2)}
        </span>
        , where small speakers can play them. Delays this track by{" "}
        <span className={READOUT}>
          {formatLatency(HARMONICS_LATENCY_FRAMES, sampleRate)}
        </span>
        .
      </p>
    </div>
  )
}

/**
 * Stage Stack: the two-stage short form of the Mastering Limiter. A
 * saturator, then a safety limiter with no look-ahead and no delay.
 */
export function StageStackEditor({
  effect,
  bind,
}: EditorProps<StageStackParams>) {
  return (
    <div
      data-slot="stage-stack-editor"
      role="group"
      aria-label="Stage Stack settings"
      className="flex max-w-[30rem] gap-3"
    >
      <GainReductionMeter effect={effect} label="Stage Stack" className="h-28" />
      <div className="grid min-w-0 flex-1 grid-cols-[2fr_1fr_1fr] items-start gap-2">
        <ParamGroup title="1 Saturate">
          <ParamRow columns={2}>
            <ParamControl
              {...bind("driveDb")}
              description="Gain into the saturator. More drive rounds the peaks harder"
            />
            <ParamControl
              {...bind("tone")}
              label="Tone"
              description="From odd harmonics only to added even harmonics"
            />
          </ParamRow>
        </ParamGroup>
        <ParamGroup title="2 Limit">
          <ParamRow columns={1}>
            <ParamControl
              {...bind("ceilingDb")}
              description="No saturated sample gets past this level. The limiter acts at once and recovers in 3 ms"
            />
          </ParamRow>
        </ParamGroup>
        <ParamGroup title="Blend">
          <ParamRow columns={1}>
            <ParamControl
              {...bind("mix")}
              description="Balance between the untouched signal and the two stages. The untouched part is not held to the ceiling"
            />
          </ParamRow>
        </ParamGroup>
      </div>
    </div>
  )
}

/**
 * The Mastering Limiter: gain in, then the three stages in the order the
 * signal meets them, each with its own controls.
 */
export function MasterLimiterEditor({
  effect,
  bind,
}: EditorProps<MasterLimiterParams>) {
  const sampleRate = useSampleRate()
  return (
    <div
      data-slot="master-limiter-editor"
      role="group"
      aria-label="Mastering Limiter settings"
      className="flex max-w-[40rem] gap-3"
    >
      <div className="flex shrink-0 flex-col items-center gap-2">
        <ParamControl
          {...bind("gainDb")}
          size="lg"
          description="Turn it up to make the mix louder. The three stages decide how the extra level is held"
        />
        <GainReductionMeter
          effect={effect}
          label="Mastering Limiter"
          className="h-24"
        />
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="grid grid-cols-[1fr_2fr_3fr] items-start gap-2">
          <ParamGroup title="1 Level">
            <ParamRow columns={1}>
              <ParamControl
                {...bind("leveling")}
                description="How much of the work the slow stage takes. It eases sustained level down and leaves the shape of each hit alone"
              />
            </ParamRow>
          </ParamGroup>
          <ParamGroup title="2 Color">
            <ParamRow columns={2}>
              <ParamControl
                {...bind("saturation")}
                description="How much of the saturation curve is blended in to round what still stands out"
              />
              <ParamControl
                {...bind("tone")}
                label="Tone"
                description="From odd harmonics only to added even harmonics"
              />
            </ParamRow>
          </ParamGroup>
          <ParamGroup title="3 Peak">
            <ParamRow columns={3}>
              <ParamControl
                {...bind("ceilingDb")}
                description="Nothing gets past this level"
              />
              <ParamControl
                {...bind("releaseMs")}
                description="How quickly the gain comes back after a peak"
              />
              <ParamControl
                {...bind("truePeak")}
                description="Holds the peak of the waveform between the samples to the ceiling, not only the samples"
              />
            </ParamRow>
          </ParamGroup>
        </div>
        <p data-slot="master-limiter-latency" className={NOTE}>
          One gain for the whole band and both channels at every stage, so
          the tonal balance and the stereo image stay as they came in. Looking
          ahead delays this track by{" "}
          <span className={READOUT}>
            {formatLatency(masterLimiterLatencyFrames(sampleRate), sampleRate)}
          </span>
          .
        </p>
      </div>
    </div>
  )
}
