import type { LimiterParams } from "@/bindings"
import { useEffectAnalyzer } from "@/features/analyzers/effect-slot"
import { AnalyzerStatusLine } from "@/features/analyzers/status"
import { ParamControl, ParamRow } from "@/features/params"
import { cn } from "@/lib/utils"

import type { EditorProps } from "../editor-props"
import { GainReductionMeter } from "../gain-reduction"
import { useSampleRate } from "../sample-rate"
import { LimiterHistory } from "./history"
import { formatLatency, lookaheadFrames } from "./latency"
import { translatedText as t } from "@/lib/i18n"

/** A gate threshold at or below this is the gate switched off. */
export const GATE_OFF_DB = -90

/**
 * The limiter: a scrolling picture of what it is doing, how hard the signal
 * is driven and the level nothing gets past, then what it does before the
 * ceiling. In compress mode the signal is compressed above a threshold
 * first; a gate, when its threshold is raised off the bottom, silences
 * what falls under it. It is the one effect that delays its track, so it
 * says by how much.
 */
export function LimiterEditor({
  effect,
  track,
  bind,
}: EditorProps<LimiterParams>) {
  const sampleRate = useSampleRate()
  const frames = lookaheadFrames(bind.value("lookaheadMs"), sampleRate)
  // The mode is a choice: 0 limits, 1 compresses first.
  const compressing = bind.value("mode") >= 0.5
  const gated = bind.value("gateThresholdDb") > GATE_OFF_DB
  const slot = useEffectAnalyzer(track, effect, { bound: true })

  return (
    <div data-slot="limiter-editor" className="flex max-w-[40rem] gap-3">
      <GainReductionMeter effect={effect} label={t("Limiter")} className="h-28" />
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="flex flex-col gap-1">
          <LimiterHistory slot={slot} />
          <AnalyzerStatusLine
            slot={slot}
            location={t("input, output and reduction as the limiter measured them")}
          />
        </div>
        <ParamRow columns={4}>
          <ParamControl
            {...bind("inputGainDb")}
            label="Input"
            description="Turn it up to push the signal into the ceiling and make it louder"
          />
          <ParamControl
            {...bind("ceilingDb")}
            description="No output sample gets past this level"
          />
          <ParamControl
            {...bind("releaseMs")}
            description="How quickly the gain comes back after a peak. Short is louder but can distort bass"
          />
          <ParamControl
            {...bind("lookaheadMs")}
            description="How far ahead it sees a peak coming. Longer distorts less and delays the track more"
          />
        </ParamRow>
        <ParamControl
          {...bind("mode")}
          label={null}
          className="w-full"
          description="Limit holds peaks under the ceiling. Compress turns down what is over the threshold first"
        />
        <section
          aria-label={t("Compression")}
          data-slot="limiter-compression"
          data-active={compressing ? "" : undefined}
          className={cn(!compressing && "opacity-60")}
        >
          <ParamRow columns={4}>
            <ParamControl
              {...bind("thresholdDb")}
              description="In compress mode, the level above which the signal is turned down"
            />
            <ParamControl
              {...bind("ratio")}
              description="In compress mode, how hard it is turned down. At the top nothing gets louder than the threshold"
            />
            <ParamControl
              {...bind("kneeDb")}
              description="In compress mode, how wide the region around the threshold is where the ratio eases in"
            />
            <ParamControl
              {...bind("attackMs")}
              description="In compress mode, how quickly the gain drops when the signal gets louder"
            />
          </ParamRow>
        </section>
        <section
          aria-label={t("Gate")}
          data-slot="limiter-gate"
          data-active={gated ? "" : undefined}
        >
          <ParamRow columns={4}>
            <ParamControl
              {...bind("gateThresholdDb")}
              description="What falls under this level is silenced. All the way down, there is no gate"
            />
            <ParamControl
              {...bind("gateReleaseMs")}
              label="Close"
              description="How quickly the gate shuts once the signal has fallen under its threshold"
            />
          </ParamRow>
        </section>
        <p
          data-slot="limiter-latency"
          className="text-[10px] leading-snug text-muted-foreground"
        >
          {t("Looking ahead delays this track by")}{" "}
          <span className="font-readout text-foreground/85">
            {formatLatency(frames, sampleRate)}
          </span>
          {t(". The engine delays every other track to match.")}
        </p>
      </div>
    </div>
  )
}
