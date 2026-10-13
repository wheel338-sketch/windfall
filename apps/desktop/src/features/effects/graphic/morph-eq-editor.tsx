import { useState } from "react"

import type { MorphEqParams } from "@/bindings"
import { Button } from "@/components/ui/button"
import {
  effectDescriptor,
  ParamControl,
  ParamGroup,
  ParamRow,
} from "@/features/params"

import { ParamFader } from "../controls/param-fader"
import type { EditorProps } from "../editor-props"
import { seriesResponseDb } from "../response/curve"
import {
  ResponseDisplay,
  type ResponseCurve,
} from "../response/response-display"
import { shownParams } from "../shown-params"
import {
  curveGainId,
  curveSections,
  describeMorph,
  interpolatedCurve,
  MORPH_CURVE_HZ,
  MORPH_CURVE_LABELS,
  MORPH_CURVE_NAMES,
  morphEqSections,
} from "./morph-eq"
import { describeBands } from "./seven-band"
import { translatedText as t, message } from "@/lib/i18n"

const descriptor = effectDescriptor("morphEq")

const BELLS = [0, 1, 2, 3] as const
const SNAPSHOTS = [
  { key: "snapshotA", get title() { return t("Bells at the start") } },
  { key: "snapshotB", get title() { return t("Bells at the end") } },
] as const

/**
 * The morphing graphic equalizer. Up to four curves are stored, each drawn
 * on fifteen fixed-frequency faders; the Morph control moves the sound from
 * one stored curve to the next, and is the one to automate. The display
 * shows the stored curves faintly and what is heard now on top.
 */
export function MorphEqEditor({ params, bind }: EditorProps<MorphEqParams>) {
  const [selected, setSelected] = useState(0)
  const [bells, setBells] = useState(false)
  const shown = shownParams(descriptor, params, bind)
  const count = shown.curveCount
  const curve = Math.min(selected, count - 1)
  const stored = shown.curves.map((item) => item.gainsDb)
  const heard = interpolatedCurve(stored, count, shown.morph)
  const where = describeMorph(shown.morph, count)

  const response: ResponseCurve = (grid, sampleRate) =>
    seriesResponseDb(morphEqSections(shown, sampleRate), grid)
  const ghosts: ResponseCurve[] = stored
    .slice(0, count)
    .map(
      (gains) => (grid, sampleRate) =>
        seriesResponseDb(curveSections(gains, sampleRate), grid)
    )

  // Several bands of the selected curve at once, as one undo step.
  const write = (key: string, gains: readonly number[]) => {
    const group = bind.group(key)
    group.onGestureStart()
    gains.forEach((gain, band) => group.set(curveGainId(curve, band), gain))
    group.onGestureEnd()
  }
  const name = MORPH_CURVE_NAMES[curve]

  return (
    <div
      data-slot="morph-eq-editor"
      role="group"
      aria-label={t("Morph EQ settings")}
      className="flex min-w-0 flex-col gap-2"
    >
      <ResponseDisplay
        response={response}
        ghosts={ghosts}
        label={message("Morph EQ response at {where}: {bands}", { where, bands: describeBands(MORPH_CURVE_HZ, heard) })}
        className="h-24"
      />
      <div className="flex min-w-0 items-end gap-3">
        <div className="min-w-0 flex-1">
          <ParamFader
            binding={bind("morph")}
            orientation="horizontal"
            label={
              <span data-slot="morph-position">{t("Morph:")}{" "}{where.toLowerCase()}</span>
            }
            description="Moves the sound from one stored curve to the next. This is the control to automate"
          />
        </div>
        <ParamControl
          {...bind("curveCount")}
          size="sm"
          description="How many of the stored curves the morph passes through, from A"
        />
      </div>
      <div className="flex min-w-0 flex-wrap items-center gap-1">
        <div
          role="group"
          aria-label={t("Stored curve")}
          className="flex items-center gap-1"
        >
          {MORPH_CURVE_NAMES.map((letter, index) => (
            <Button
              key={letter}
              size="xs"
              variant={index === curve ? "secondary" : "ghost"}
              aria-pressed={index === curve}
              aria-label={message("Curve {letter}", { letter })}
              disabled={index >= count}
              onClick={() => setSelected(index)}
            >
              {letter}
            </Button>
          ))}
        </div>
        <span className="flex-1" />
        <Button
          size="xs"
          variant="outline"
          disabled={bind.disabled}
          onClick={() => write("capture", heard)}
        >
          {t("Store what is heard in")}{" "}{name}
        </Button>
        <Button
          size="xs"
          variant="outline"
          disabled={bind.disabled || stored[curve].every((gain) => gain === 0)}
          onClick={() =>
            write(
              "flat",
              MORPH_CURVE_HZ.map(() => 0)
            )
          }
        >
          {t("Flat")}
        </Button>
      </div>
      <div
        role="group"
        aria-label={message("Curve {name}", { name })}
        className="grid min-w-0 grid-cols-[repeat(15,minmax(0,1fr))] justify-items-center gap-0.5"
      >
        {MORPH_CURVE_HZ.map((_, band) => (
          <ParamFader
            key={curveGainId(curve, band)}
            binding={bind(curveGainId(curve, band))}
            label={MORPH_CURVE_LABELS[band]}
            description={message("Boost or cut around this frequency in curve {name}", { name })}
          />
        ))}
      </div>
      <div>
        <Button
          size="xs"
          variant="ghost"
          aria-expanded={bells}
          onClick={() => setBells(!bells)}
        >
          {t("Movable bells")}
        </Button>
      </div>
      {bells ? (
        <div className="grid grid-cols-[repeat(auto-fit,minmax(13rem,1fr))] items-start gap-2">
          {SNAPSHOTS.map((snapshot) => (
            <ParamGroup key={snapshot.key} title={snapshot.title}>
              {BELLS.map((bell) => (
                <ParamRow key={bell} columns={3}>
                  <ParamControl
                    {...bind(`${snapshot.key}.bands.${bell}.frequencyHz`)}
                    size="sm"
                    label={`${bell + 1} Hz`}
                  />
                  <ParamControl
                    {...bind(`${snapshot.key}.bands.${bell}.gainDb`)}
                    size="sm"
                    label="Gain"
                  />
                  <ParamControl
                    {...bind(`${snapshot.key}.bands.${bell}.q`)}
                    size="sm"
                    label="Q"
                  />
                </ParamRow>
              ))}
            </ParamGroup>
          ))}
        </div>
      ) : null}
    </div>
  )
}
