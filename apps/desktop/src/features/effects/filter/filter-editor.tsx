import type { SelectableFilterParams } from "@/bindings"
import { formatHz } from "@/components/audio"
import { ParamControl, ParamGroup, ParamRow } from "@/features/params"

import type { EditorProps } from "../editor-props"
import { bandPass, highPass, highShelf, lowPass, lowShelf, notch, peak, seriesResponseDb } from "../response/curve"
import { ResponseDisplay, type ResponseCurve } from "../response/response-display"
import { translatedText as t, message } from "@/lib/i18n"

/** A resonant sweep, with its transfer shown and automation on every control. */
export function FilterEditor({ bind }: EditorProps<SelectableFilterParams>) {
  const mode = bind.value("mode")
  const hz = bind.value("frequencyHz")
  const q = bind.value("q")
  const gain = bind.value("gainDb")
  const info = bind.info("mode")
  const response: ResponseCurve = (grid, rate) => {
    const section = mode === 0 ? lowPass(hz, q, rate)
      : mode === 1 ? highPass(hz, q, rate)
      : mode === 2 ? bandPass(hz, q, rate)
      : mode === 3 ? notch(hz, q, rate)
      : mode === 4 ? lowShelf(hz, q, gain, rate)
      : mode === 5 ? peak(hz, q, gain, rate)
      : highShelf(hz, q, gain, rate)
    return seriesResponseDb([section], grid)
  }
  return (
    <div role="group" aria-label={t("Selectable filter settings")} className="flex min-w-0 flex-col gap-2">
      <ParamControl {...bind("mode")} choiceStyle="select" layout="inline" />
      <ResponseDisplay response={response} rangeDb={24}
        label={message("Resonant filter: {choicesLabel}, {hz}, Q {q}", { choicesLabel: info.choices[mode]?.label, hz: formatHz(hz), q: q.toFixed(2) })} />
      <ParamGroup title="Sweep">
        <ParamRow columns={3}>
          <ParamControl {...bind("frequencyHz")} description="Sweep the cutoff or center; right-click to draw an automation curve" />
          <ParamControl {...bind("q")} description="Higher resonance emphasizes the cutoff and narrows a bandpass" />
          <ParamControl {...bind("gainDb")} description="Gain applies to shelves and the peaking mode" />
        </ParamRow>
      </ParamGroup>
    </div>
  )
}
