import { useRef, type ReactNode } from "react"

import { useEffectAnalyzer } from "@/features/analyzers/effect-slot"
import { useAnalyzerFrames } from "@/features/analyzers/feed"
import { formatCents, noteName } from "@/features/analyzers/math"
import { AnalyzerStatusLine } from "@/features/analyzers/status"
import type { AnalyzerObservation } from "@/features/analyzers/types"
import { ParamControl, type EffectParamsOf } from "@/features/params"
import { clamp } from "@/lib/units"

import type { EditorProps } from "../editor-props"
import { formatNumber, translatedText as t } from "@/lib/i18n"

/** How far the needle travels either side of the note, in cents. */
export const TUNER_RANGE_CENTS = 50
/** Within this many cents the note counts as in tune. */
export const IN_TUNE_CENTS = 5

type TunerReading = Extract<AnalyzerObservation, { type: "tuner" }>

/** What a reading says, for the display and for a screen reader. */
export function describeReading(reading: TunerReading | null): {
  note: string
  cents: string
  frequency: string
  spoken: string
  /** The needle's place, 0 to 1, or null with no pitch. */
  needle: number | null
  inTune: boolean
} {
  if (
    !reading ||
    reading.midiNote === null ||
    reading.cents === null ||
    reading.frequencyHz === null
  ) {
    return {
      note: "–",
      cents: "",
      frequency: "",
      spoken: t("No steady pitch"),
      needle: null,
      inTune: false,
    }
  }
  const note = noteName(reading.midiNote)
  const cents = formatCents(reading.cents)
  const inTune = Math.abs(reading.cents) <= IN_TUNE_CENTS
  const side = inTune ? t("in tune") : reading.cents > 0 ? t("sharp") : t("flat")
  return {
    note,
    cents,
    frequency: `${formatNumber(reading.frequencyHz, { minimumFractionDigits: 1, maximumFractionDigits: 1, useGrouping: false })} Hz`,
    spoken: [note, cents, side].join(", "),
    needle:
      (clamp(reading.cents, -TUNER_RANGE_CENTS, TUNER_RANGE_CENTS) +
        TUNER_RANGE_CENTS) /
      (2 * TUNER_RANGE_CENTS),
    inTune,
  }
}

/**
 * The tuner: the note the track is playing and how far it is from that
 * note, in cents, against the reference pitch set here. The reading is the
 * tuner's own, taken from the audio that passes through it on this track;
 * it leaves that audio untouched. It reads one note at a time.
 */
export function TunerReadout({ slot, reference }: { slot: string; reference?: ReactNode }) {
  const root = useRef<HTMLDivElement>(null)
  const meter = useRef<HTMLDivElement>(null)
  const needle = useRef<HTMLSpanElement>(null)
  const note = useRef<HTMLSpanElement>(null)
  const cents = useRef<HTMLSpanElement>(null)
  const frequency = useRef<HTMLSpanElement>(null)
  const newest = useRef<TunerReading | null>(null)

  useAnalyzerFrames(slot, {
    onFrame(frame) {
      newest.current = null
      for (const observation of frame.observations) {
        if (observation.type === "tuner") newest.current = observation
      }
    },
    onClear() {
      newest.current = null
    },
    paint() {
      const shown = describeReading(newest.current)
      if (note.current) note.current.textContent = shown.note
      if (cents.current) cents.current.textContent = shown.cents
      if (frequency.current) frequency.current.textContent = shown.frequency
      if (needle.current) {
        needle.current.hidden = shown.needle === null
        if (shown.needle !== null) {
          needle.current.style.left = `${(shown.needle * 100).toFixed(2)}%`
        }
      }
      if (meter.current) {
        meter.current.setAttribute("aria-valuetext", shown.spoken)
        const value = newest.current?.cents
        if (shown.needle !== null && value !== null && value !== undefined) {
          meter.current.setAttribute("aria-valuenow", value.toFixed(1))
        } else {
          meter.current.removeAttribute("aria-valuenow")
        }
      }
      if (root.current) {
        root.current.dataset.note = shown.note
        root.current.dataset.cents = newest.current?.cents?.toFixed(2) ?? ""
        root.current.dataset.frequencyHz = newest.current?.frequencyHz?.toFixed(2) ?? ""
        root.current.toggleAttribute("data-in-tune", shown.inTune)
        root.current.toggleAttribute("data-pitched", shown.needle !== null)
      }
    },
  })

  return (
    <div
      ref={root}
      data-slot="tuner-editor"
      className="group/tuner flex max-w-[30rem] items-stretch gap-3"
    >
      <div className="flex min-w-0 flex-1 flex-col gap-1.5">
        <div className="flex items-baseline justify-between gap-3 rounded-md bg-display px-3 py-2 font-readout tabular-nums">
          <span
            ref={note}
            data-slot="tuner-note"
            className="text-3xl leading-none text-display-foreground group-data-in-tune/tuner:text-(--wf-meter-low)"
          >
            –
          </span>
          <span className="flex flex-col items-end gap-0.5 text-xs text-display-dim">
            <span ref={cents} data-slot="tuner-cents" />
            <span ref={frequency} data-slot="tuner-frequency" />
          </span>
        </div>
        <div
          ref={meter}
          role="meter"
          aria-label={t("Cents from the nearest note")}
          aria-valuemin={-TUNER_RANGE_CENTS}
          aria-valuemax={TUNER_RANGE_CENTS}
          aria-valuetext={t("No steady pitch")}
          data-slot="tuner-meter"
          className="relative h-3 rounded-sm bg-(--wf-meter-bg)"
        >
          <span
            aria-hidden
            className="absolute inset-y-0 left-1/2 w-px bg-foreground/60"
          />
          <span
            ref={needle}
            hidden
            aria-hidden
            className="absolute inset-y-0 w-1 -translate-x-1/2 rounded-sm bg-(--wf-meter-mid) group-data-in-tune/tuner:bg-(--wf-meter-low)"
          />
        </div>
        <div
          aria-hidden
          className="flex justify-between font-readout text-[9px] text-muted-foreground"
        >
          <span>−{TUNER_RANGE_CENTS}{" "}{t("¢ flat")}</span>
          <span>{t("in tune")}</span>
          <span>{t("sharp +")}{TUNER_RANGE_CENTS} ¢</span>
        </div>
        <AnalyzerStatusLine slot={slot} location={t("this track, one note at a time")} />
      </div>
      <div className="flex shrink-0 flex-col justify-center rounded-md border border-border/80 bg-chassis/50 px-2 py-2">
        {reference}
      </div>
    </div>
  )
}

export function TunerEditor({ effect, track, bind }: EditorProps<EffectParamsOf<"tuner">>) {
  const slot = useEffectAnalyzer(track, effect, { bound: true })
  return <TunerReadout slot={slot} reference={
    <ParamControl {...bind("referenceHz")} label="A4"
      description="The pitch of the A above middle C that notes are measured against" />
  } />
}
