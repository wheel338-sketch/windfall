import { useMemo } from "react"

import type { CompressorParams } from "@/bindings"
import { formatDb } from "@/components/audio"
import { ParamControl, ParamRow } from "@/features/params"

import type { EditorProps } from "../editor-props"
import { GainReductionMeter } from "../gain-reduction"
import { totalMakeupDb } from "./curve"
import { TransferDisplay } from "./transfer-display"
import { translatedText as t } from "@/lib/i18n"

/**
 * The compressor: its curve and how far it is turning the signal down
 * right now, then the controls that shape the curve and the ones that time
 * it. In a narrow panel the timing controls go under the curve; given
 * 26rem or more, every control stands beside it, so the whole editor is in
 * view in a mixer of ordinary height.
 */
export function CompressorEditor({
  effect,
  bind,
}: EditorProps<CompressorParams>) {
  const thresholdDb = bind.value("thresholdDb")
  const ratio = bind.value("ratio")
  const kneeDb = bind.value("kneeDb")
  const makeupDb = bind.value("makeupDb")
  const autoMakeup = bind.value("autoMakeup") >= 0.5
  const mix = bind.value("mix")
  // What the controls show, which is ahead of the project during a drag.
  const shown = useMemo(
    () => ({ thresholdDb, ratio, kneeDb, makeupDb, autoMakeup, mix }),
    [thresholdDb, ratio, kneeDb, makeupDb, autoMakeup, mix]
  )

  return (
    <div
      data-slot="compressor-editor"
      className="grid max-w-[40rem] grid-cols-[auto_minmax(0,1fr)] gap-2.5"
    >
      <div className="flex items-stretch gap-2.5 @min-[26rem]/editor:row-span-3 @min-[26rem]/editor:items-start">
        <TransferDisplay
          params={shown}
          className="w-32 shrink-0 in-data-enlarged:w-48"
        />
        <GainReductionMeter
          effect={effect}
          label={t("Compressor")}
          // The enlarged height carries the docked rule's container query, so
          // it is the later rule of the two and wins.
          className="@min-[26rem]/editor:h-32 in-data-enlarged:@min-[26rem]/editor:h-48"
        />
      </div>
      <div className="flex min-w-0 flex-col justify-between gap-2 @min-[26rem]/editor:flex-row @min-[26rem]/editor:items-center">
        <ParamRow
          columns={2}
          className="@min-[26rem]/editor:w-28 @min-[26rem]/editor:shrink-0"
        >
          <ParamControl
            {...bind("thresholdDb")}
            description="The level above which the signal is turned down"
          />
          <ParamControl
            {...bind("ratio")}
            description="How hard it is turned down. At the top nothing gets louder than the threshold"
          />
        </ParamRow>
        <ParamControl
          {...bind("detector")}
          label={null}
          className="w-full"
          description="Peak reacts to the loudest sample, RMS to the average over about 10 ms"
        />
      </div>
      <ParamControl {...bind("sidechain")} layout="inline" className="col-span-2" description="Use the mixer's detector-only key input. With no key signal, gain reduction releases." />
      <ParamRow
        columns={5}
        className="col-span-2 @min-[26rem]/editor:col-span-1 @min-[26rem]/editor:col-start-2"
      >
        <ParamControl
          {...bind("attackMs")}
          description="How quickly the gain drops when the signal gets louder"
        />
        <ParamControl
          {...bind("releaseMs")}
          description="How quickly the gain recovers when the signal gets quieter"
        />
        <ParamControl
          {...bind("kneeDb")}
          description="How wide the region around the threshold is where the ratio eases in. 0 is a hard corner"
        />
        <ParamControl
          {...bind("makeupDb")}
          description="Gain added after compression"
        />
        <ParamControl
          {...bind("mix")}
          description="Balance between the untouched signal and the compressed one"
        />
      </ParamRow>
      <div className="col-span-2 flex items-center justify-between gap-3 @min-[26rem]/editor:col-span-1 @min-[26rem]/editor:col-start-2">
        <ParamControl
          {...bind("autoMakeup")}
          layout="inline"
          className="gap-3"
          description="Adds half of what a full-scale signal is turned down by"
        />
        <span
          data-slot="compressor-makeup"
          className="font-readout text-[10px] text-muted-foreground"
        >
          <span className="sr-only">{t("Makeup in effect:")}{" "}</span>
          {formatDb(totalMakeupDb(shown))}{" "}{t("makeup")}
        </span>
      </div>
    </div>
  )
}
