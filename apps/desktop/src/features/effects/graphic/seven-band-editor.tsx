import type { SevenBandParams } from "@/bindings"
import { Button } from "@/components/ui/button"

import { ParamFader } from "../controls/param-fader"
import type { EditorProps } from "../editor-props"
import { seriesResponseDb } from "../response/curve"
import {
  ResponseDisplay,
  type ResponseCurve,
} from "../response/response-display"
import {
  describeBands,
  SEVEN_BAND_HZ,
  SEVEN_BAND_IDS,
  SEVEN_BAND_LABELS,
  sevenBandSections,
} from "./seven-band"
import { translatedText as t, message } from "@/lib/i18n"

/**
 * The fixed-band graphic equalizer: seven faders at fixed frequencies, read
 * left to right as the curve they make, with that curve drawn above them.
 * There is nothing else to set, which is the point of it.
 */
export function SevenBandEditor({ bind }: EditorProps<SevenBandParams>) {
  const gains = SEVEN_BAND_IDS.map((id) => bind.value(id))
  const flat = gains.every((gain) => gain === 0)
  const response: ResponseCurve = (grid, sampleRate) =>
    seriesResponseDb(sevenBandSections(gains, sampleRate), grid)

  // Every band back to 0 dB, as one undo step.
  const flatten = () => {
    const group = bind.group("flat")
    group.onGestureStart()
    for (const id of SEVEN_BAND_IDS) group.set(id, 0)
    group.onGestureEnd()
  }

  return (
    <div
      data-slot="seven-band-editor"
      role="group"
      aria-label={t("Seven Band settings")}
      className="flex min-w-0 flex-col gap-2"
    >
      <ResponseDisplay
        response={response}
        label={message("Seven Band response: {bands}", { bands: describeBands(SEVEN_BAND_HZ, gains) })}
        className="h-24"
      />
      <div className="flex items-end gap-2">
        <div className="grid min-w-0 flex-1 grid-cols-7 justify-items-center gap-1">
          {SEVEN_BAND_IDS.map((id, index) => (
            <ParamFader
              key={id}
              binding={bind(id)}
              label={SEVEN_BAND_LABELS[index]}
              description="Boost or cut around this frequency. The frequency is fixed"
            />
          ))}
        </div>
        <Button
          size="xs"
          variant="outline"
          disabled={flat || bind.disabled}
          onClick={flatten}
        >
          {t("Flat")}
        </Button>
      </div>
    </div>
  )
}
