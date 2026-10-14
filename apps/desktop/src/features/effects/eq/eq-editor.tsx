import type { CSSProperties, KeyboardEvent } from "react"

import type { EqParams } from "@/bindings"
import { useEffectAnalyzer } from "@/features/analyzers/effect-slot"
import { AnalyzerStatusLine } from "@/features/analyzers/status"
import { effectDescriptor, ParamControl, ParamRow } from "@/features/params"
import { cn } from "@/lib/utils"

import type { EditorProps } from "../editor-props"
import { useSampleRate } from "../sample-rate"
import { shownParams } from "../shown-params"
import { BAND_COLOR_SCOPE, bandSpec, EQ_BANDS, type BandSpec } from "./bands"
import { EqDisplay } from "./eq-display"
import type { EqBandId, EqBandShape } from "./eq-response"
import { selectEqBand, setEqRange, useEqBand, useEqRange } from "./eq-view"
import { DB_RANGES } from "./geometry"
import { EqLiveSpectrum } from "./live-spectrum"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const descriptor = effectDescriptor("eq")

/*
 * Where each part goes. Narrow, in rows: the curve, then the band buttons
 * with the range beside them, then the band's controls with the output
 * beside them. Wide, the curve takes the left and the rest is a column on
 * the right: band buttons, the band's controls, then range and output. The
 * parts are written in the order of the wide layout, and the narrow one
 * moves the range up beside the band buttons.
 */
const WIDE = {
  root: "@min-[26rem]/editor:grid-cols-[minmax(0,1fr)_auto_auto] @min-[26rem]/editor:items-stretch",
  display:
    "@min-[26rem]/editor:col-span-1 @min-[26rem]/editor:row-span-3 @min-[26rem]/editor:h-full @min-[26rem]/editor:min-h-44 in-data-enlarged:min-h-72!",
  bands: "@min-[26rem]/editor:col-span-2",
  band: "order-3 @min-[26rem]/editor:order-none @min-[26rem]/editor:col-span-2 @min-[26rem]/editor:w-48 @min-[40rem]/editor:w-60",
  range: "order-2 @min-[26rem]/editor:order-none",
  output: "order-4 @min-[26rem]/editor:order-none",
} as const

/** The response of each kind of band, drawn 16 by 10. */
const SHAPE_PATHS: Record<EqBandShape, string> = {
  lowCut: "M1 9 C4 9 5 2.5 8 2.5 L15 2.5",
  lowShelf: "M1 3 L4.5 3 C8 3 8 7.5 11.5 7.5 L15 7.5",
  peak: "M1 8 C5 8 6 2 8 2 C10 2 11 8 15 8",
  highShelf: "M1 7.5 L4.5 7.5 C8 7.5 8 3 11.5 3 L15 3",
  highCut: "M1 2.5 L8 2.5 C11 2.5 12 9 15 9",
}

function ShapeIcon({ shape }: { shape: EqBandShape }) {
  return (
    <svg width="16" height="10" viewBox="0 0 16 10" aria-hidden>
      <path
        d={SHAPE_PATHS[shape]}
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
    </svg>
  )
}

/** Moves a radio group's choice with the arrow keys. */
function arrowStep(event: KeyboardEvent, index: number, count: number) {
  if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
    return null
  }
  if (event.key === "ArrowRight" || event.key === "ArrowDown") {
    return (index + 1) % count
  }
  if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
    return (index + count - 1) % count
  }
  return null
}

type BandTabsProps = {
  params: EqParams
  selected: EqBandId
  onSelect(band: EqBandId): void
}

/** One button per band, to pick the band the controls below belong to. */
function BandTabs({ params, selected, onSelect }: BandTabsProps) {
  const index = EQ_BANDS.findIndex((band) => band.id === selected)
  return (
    <div
      role="radiogroup"
      aria-label={t("Band")}
      data-slot="eq-bands"
      className={cn(
        "flex min-w-0 flex-1 rounded-[5px] bg-(--wf-step-off)/70 p-px",
        WIDE.bands
      )}
      onKeyDown={(event) => {
        const next = arrowStep(event, index, EQ_BANDS.length)
        if (next === null) return
        event.preventDefault()
        onSelect(EQ_BANDS[next].id)
        event.currentTarget
          .querySelector<HTMLElement>(`[data-band="${EQ_BANDS[next].id}"]`)
          ?.focus()
      }}
    >
      {EQ_BANDS.map((band) => {
        const chosen = band.id === selected
        return (
          <button
            key={band.id}
            type="button"
            role="radio"
            aria-checked={chosen}
            aria-label={band.name}
            title={band.name}
            data-band={band.id}
            data-off={params[band.id].enabled ? undefined : ""}
            tabIndex={chosen ? 0 : -1}
            onClick={() => onSelect(band.id)}
            className={cn(
              "flex h-5 min-w-0 flex-1 items-center justify-center rounded-[4px] text-(--band) outline-none hover:bg-background/60 focus-visible:ring-2 focus-visible:ring-ring data-off:text-muted-foreground/70",
              chosen && "bg-background shadow-xs"
            )}
            style={{ "--band": band.color } as CSSProperties}
          >
            <ShapeIcon shape={band.shape} />
          </button>
        )
      })}
    </div>
  )
}

type RangeTabsProps = { range: number; onChange(range: number): void }

/** How many dB the display shows either side of 0. */
function RangeTabs({ range, onChange }: RangeTabsProps) {
  const index = DB_RANGES.findIndex((item) => item === range)
  return (
    <div
      role="radiogroup"
      aria-label={t("Gain range of the display")}
      data-slot="eq-range"
      className={cn(
        "flex shrink-0 self-center rounded-[5px] bg-(--wf-step-off)/70 p-px",
        WIDE.range
      )}
      onKeyDown={(event) => {
        const next = arrowStep(event, index, DB_RANGES.length)
        if (next === null) return
        event.preventDefault()
        onChange(DB_RANGES[next])
        event.currentTarget
          .querySelector<HTMLElement>(`[data-range="${DB_RANGES[next]}"]`)
          ?.focus()
      }}
    >
      {DB_RANGES.map((item) => (
        <button
          key={item}
          type="button"
          role="radio"
          aria-checked={item === range}
          aria-label={message("Show {item} dB either side of 0", { item: plainNumber(item) })}
          data-range={item}
          tabIndex={item === range ? 0 : -1}
          onClick={() => onChange(item)}
          className={cn(
            "h-5 rounded-[4px] px-1.5 font-readout text-[9px] leading-none text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring",
            item === range && "bg-background text-foreground shadow-xs"
          )}
        >
          ±{item}
        </button>
      ))}
    </div>
  )
}

type BandControlsProps = {
  spec: BandSpec
  bind: EditorProps<EqParams>["bind"]
}

/** The settings of one band: on or off, frequency, gain or slope, and Q. */
function BandControls({ spec, bind }: BandControlsProps) {
  return (
    <section
      aria-label={spec.name}
      data-slot="eq-band"
      className={cn(
        "flex min-w-0 flex-1 flex-col gap-1.5 rounded-md border border-border/80 bg-chassis/50 px-2 pt-1.5 pb-2",
        WIDE.band
      )}
    >
      <h4 className="flex items-center gap-1.5 text-[0.6875rem] leading-none font-medium text-foreground/80">
        <span
          aria-hidden
          className="size-2 rounded-full"
          style={{ backgroundColor: spec.color }}
        />
        {spec.name}
      </h4>
      <ParamRow columns={4}>
        <ParamControl
          {...bind(spec.enabled)}
          label={null}
          color={spec.color}
          description="A band that is off leaves the signal alone"
        />
        <ParamControl
          {...bind(spec.frequency)}
          label="Freq"
          color={spec.color}
        />
        {spec.gain !== null && (
          <ParamControl {...bind(spec.gain)} label="Gain" color={spec.color} />
        )}
        {spec.slope !== null && (
          <ParamControl
            {...bind(spec.slope)}
            label="Slope"
            choiceStyle="select"
            className="w-full self-center"
            description="How steeply the filter falls past its corner"
          />
        )}
        <ParamControl
          {...bind(spec.q)}
          label="Q"
          color={spec.color}
          description={
            spec.shape === "peak"
              ? "Higher is narrower"
              : spec.gain !== null
                ? "0.71 is the smoothest slope. Higher adds a bump before the shelf"
                : "0.71 is flat up to the corner. Higher adds a peak there"
          }
        />
      </ParamRow>
    </section>
  )
}

/**
 * The parametric equaliser: its curve with a node per band, the controls of
 * the band that is picked and the output gain. In a narrow panel the
 * controls are under the curve; given 26rem or more they stand beside it,
 * so the whole editor is in view in a mixer of ordinary height.
 */
export function EqEditor({
  effect,
  track,
  params,
  bind,
}: EditorProps<EqParams>) {
  const sampleRate = useSampleRate()
  // The spectrum is the track's own audio after its fader, analyzed
  // natively. The curve stays what the settings work out to.
  const slot = useEffectAnalyzer(track, effect, {
    bound: false,
    want: { spectrum: true },
  })
  const band = useEqBand(effect)
  const range = useEqRange(effect)
  const shown = shownParams(descriptor, params, bind)
  const select = (next: EqBandId) => selectEqBand(effect, next)

  return (
    <div
      data-slot="eq-editor"
      className={cn(
        // A little closer together docked, where every pixel of height is
        // one less to scroll.
        "grid grid-cols-[minmax(0,1fr)_auto] gap-1.5 in-data-enlarged:gap-2",
        WIDE.root,
        BAND_COLOR_SCOPE
      )}
    >
      <EqDisplay
        params={shown}
        sampleRate={sampleRate}
        range={range}
        selected={band}
        onSelect={select}
        bind={bind}
        underlay={<EqLiveSpectrum slot={slot} />}
        className={cn("col-span-2", WIDE.display)}
      />
      <BandTabs params={shown} selected={band} onSelect={select} />
      {/* Keyed by band, so a drag in flight never lands on another one. */}
      <BandControls key={band} spec={bandSpec(band)} bind={bind} />
      <RangeTabs
        range={range}
        onChange={(next) => {
          const found = DB_RANGES.find((item) => item === next)
          if (found !== undefined) setEqRange(effect, found)
        }}
      />
      <div
        className={cn(
          "flex shrink-0 flex-col justify-end rounded-md border border-border/80 bg-chassis/50 px-2 pb-2",
          WIDE.output
        )}
      >
        <ParamControl
          {...bind("outputGainDb")}
          label="Output"
          description="Gain applied after all the bands"
        />
      </div>
      <AnalyzerStatusLine
        slot={slot}
        location="spectrum of this track's output, −90 to 0 dB"
        className="col-span-full"
      />
    </div>
  )
}
