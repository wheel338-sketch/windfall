import type { BandSplitParams } from "@/bindings"
import { ParamControl, ParamGroup, ParamRow } from "@/features/params"

import type { EditorProps } from "../editor-props"
import { ResponseDisplay, type ResponseCurve } from "../response/response-display"
import { useSampleRate } from "../sample-rate"
import { bandResponsesDb, describeSplit } from "./bands"
import { translatedText as t, message } from "@/lib/i18n"

/** The mixer inspector adds the shared per-effect output selectors below this. */
export function BandSplitEditor({ bind }: EditorProps<BandSplitParams>) {
  const sampleRate = useSampleRate()
  const settings = {
    three: bind.value("bands") === 1,
    lowCrossoverHz: bind.value("lowCrossoverHz"),
    highCrossoverHz: bind.value("highCrossoverHz"),
    lowGainDb: bind.value("lowGainDb"),
    midGainDb: bind.value("midGainDb"),
    highGainDb: bind.value("highGainDb"),
  }
  const curves: ResponseCurve[] = [0, 1, 2].map((band) => (grid, rate) =>
    bandResponsesDb(settings, grid.frequenciesHz, rate)[band])
  return (
    <div role="group" aria-label={t("Band Split settings")} className="flex min-w-0 flex-col gap-2">
      <ParamControl {...bind("bands")} layout="inline" />
      <ResponseDisplay response={curves[0]} ghosts={settings.three ? curves.slice(1) : [curves[2]]}
        rangeDb={24} label={message("Band Split response: {split}", { split: describeSplit(settings, sampleRate) })} />
      <ParamGroup title="Crossovers">
        <ParamRow columns={2}>
          <ParamControl {...bind("lowCrossoverHz")} />
          {settings.three && <ParamControl {...bind("highCrossoverHz")} />}
        </ParamRow>
      </ParamGroup>
      <ParamGroup title="Band levels">
        <ParamRow columns={3}>
          <ParamControl {...bind("lowGainDb")} />
          {settings.three && <ParamControl {...bind("midGainDb")} />}
          <ParamControl {...bind("highGainDb")} />
        </ParamRow>
      </ParamGroup>
      <p className="text-[10px] text-muted-foreground">
        {t("Route each band below to process it on another mixer track. Unrouted bands stay in this chain. In two-band mode Mid is silent; High carries everything above the crossover.")}
      </p>
    </div>
  )
}
