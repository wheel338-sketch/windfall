import {
  useRef,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
} from "react"

import { ToggleLed } from "@/components/audio"
import { ContextActions } from "@/components/context-actions"
import { formatParamDisplay, type ParamBinder } from "@/features/params"
import { useHint } from "@/lib/store/hint"
import { cn } from "@/lib/utils"

import {
  BAND_COUNT,
  bandCenter,
  bandParam,
  fractionOf,
  LANES,
  plotPoint,
  shortHz,
  strokeBands,
  valueAt,
  type Lane,
  type PlotPoint,
} from "./bands"
import { message, translatedText } from "@/lib/i18n"

const BANDS = Array.from({ length: BAND_COUNT }, (_, band) => band)

/** Where a bar's fill sits in its column, as shares of the column's height. */
function fill(fraction: number, bipolar: boolean): CSSProperties {
  if (!bipolar) {
    return { bottom: 0, height: `${fraction * 100}%` }
  }
  // A setting with a middle is drawn from the middle, up or down.
  const low = Math.min(fraction, 0.5)
  return { bottom: `${low * 100}%`, height: `${Math.abs(fraction - 0.5) * 100}%` }
}

type BarProps = {
  band: number
  lane: Lane
  bind: ParamBinder
  bipolar: boolean
}

/**
 * One band's bar. It is a slider of its own, so the keyboard and a screen
 * reader reach every band, while the pointer draws on the whole plot.
 */
function Bar({ band, lane, bind, bipolar }: BarProps) {
  const id = bandParam(band, lane)
  const binding = bind(id)
  const { info, value } = binding
  const enabled = bind.value(bandParam(band, "enabled")) >= 0.5
  const group = bind.group(`bands.${lane}`)

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (binding.disabled || event.altKey || event.ctrlKey || event.metaKey) {
      return
    }
    const span = info.max - info.min
    const step = span / (event.shiftKey ? 1000 : 100)
    let next: number
    switch (event.key) {
      case "ArrowUp":
        next = value + step
        break
      case "ArrowDown":
        next = value - step
        break
      case "PageUp":
        next = value + span / 10
        break
      case "PageDown":
        next = value - span / 10
        break
      case "Home":
        next = info.min
        break
      case "End":
        next = info.max
        break
      case "ArrowLeft":
      case "ArrowRight": {
        // Left and right walk along the bands instead of changing one.
        event.preventDefault()
        const to = band + (event.key === "ArrowRight" ? 1 : -1)
        event.currentTarget.parentElement
          ?.querySelector<HTMLElement>(`[data-band="${to}"]`)
          ?.focus()
        return
      }
      default:
        return
    }
    event.preventDefault()
    // Outside a stroke each key press is an undo step of its own.
    group.set(id, valueAt(info, fractionOf(info, next)))
  }

  const bar = (
    <div
      role="slider"
      tabIndex={binding.disabled ? -1 : 0}
      aria-label={translatedText(info.name)}
      aria-orientation="vertical"
      aria-valuemin={info.min}
      aria-valuemax={info.max}
      aria-valuenow={value}
      aria-valuetext={formatParamDisplay(info, value)}
      aria-disabled={binding.disabled || undefined}
      data-slot="band-bar"
      data-param={id}
      data-band={band}
      data-off={enabled ? undefined : ""}
      onKeyDown={onKeyDown}
      onDoubleClick={() => {
        if (!binding.disabled) group.set(id, info.default)
      }}
      className="group/bar relative h-full min-w-0 flex-1 rounded-[2px] outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      <span
        aria-hidden
        data-slot="band-fill"
        className={cn(
          "pointer-events-none absolute inset-x-px min-h-0.5 rounded-[2px] bg-brand/80 group-hover/bar:bg-brand",
          // A band that is switched off passes its sound undelayed.
          !enabled && "bg-display-dim/60 group-hover/bar:bg-display-dim"
        )}
        style={fill(fractionOf(info, value), bipolar)}
      />
      {binding.marker === undefined ? null : (
        <span
          aria-hidden
          data-slot="param-marker"
          className="pointer-events-none absolute top-0.5 right-0.5 size-[5px] rounded-full"
          style={{ backgroundColor: binding.marker }}
        />
      )}
    </div>
  )
  // What the band's setting is bound to, such as an automation clip, is
  // offered from its bar.
  return binding.contextItems.length > 0 ? (
    <ContextActions items={() => [...binding.contextItems]}>{bar}</ContextActions>
  ) : (
    bar
  )
}

type BandPlotProps = {
  /** Which setting of the bands the bars show and the pointer draws. */
  lane: Lane
  bind: ParamBinder
}

/**
 * Sixteen bars, one per band, that are drawn on like a graph: press
 * anywhere and drag across to give every band the pointer passes the height
 * it passes at. One stroke is one undo step. Under each bar is the band's
 * frequency and its on/off light.
 */
export function BandPlot({ lane, bind }: BandPlotProps) {
  const plot = useRef<HTMLDivElement>(null)
  /** The last point of the stroke under way, or null between strokes. */
  const stroke = useRef<{ last: PlotPoint | null } | null>(null)
  const group = bind.group(`bands.${lane}`)
  const described = LANES.find((item) => item.id === lane) ?? LANES[0]
  const first = bind.info(bandParam(0, lane))
  const bipolar = first.min < 0
  const hint = useHint(
    message("{describedName} of each band. {hint}. Drag across to draw, double-click a bar to reset it", { describedName: described.name, hint: described.hint })
  )

  function draw(event: PointerEvent<HTMLDivElement>) {
    const run = stroke.current
    const box = plot.current?.getBoundingClientRect()
    if (!run || !box) return
    const to = plotPoint(event.clientX, event.clientY, box)
    if (!to) return
    for (const point of strokeBands(run.last, to)) {
      const id = bandParam(point.band, lane)
      group.set(id, valueAt(bind.info(id), point.fraction))
    }
    run.last = to
  }

  function end() {
    if (!stroke.current) return
    stroke.current = null
    group.onGestureEnd()
  }

  return (
    <div data-slot="band-plot" className="flex min-w-0 flex-col gap-1">
      <div className="flex min-w-0 items-stretch gap-1.5">
        <div
          aria-hidden
          className="flex w-12 shrink-0 flex-col justify-between text-right font-readout text-[9px] leading-none text-muted-foreground"
        >
          <span>{formatParamDisplay(first, first.max)}</span>
          {bipolar ? <span>{formatParamDisplay(first, 0)}</span> : null}
          <span>{formatParamDisplay(first, first.min)}</span>
        </div>
        <div
          ref={plot}
          role="group"
          aria-label={message("{describedName} of the sixteen bands", { describedName: described.name })}
          data-slot="band-bars"
          data-lane={lane}
          className={cn(
            "relative flex h-28 min-w-0 flex-1 touch-none items-stretch gap-px rounded-sm bg-display p-px select-none in-data-enlarged:h-44",
            bind.disabled ? "opacity-50" : "cursor-crosshair"
          )}
          onPointerDown={(event) => {
            if (event.button !== 0 || bind.disabled) return
            event.currentTarget.setPointerCapture?.(event.pointerId)
            stroke.current = { last: null }
            group.onGestureStart()
            draw(event)
          }}
          onPointerMove={draw}
          onPointerUp={end}
          onPointerCancel={end}
          onLostPointerCapture={end}
          {...hint}
        >
          {bipolar ? (
            <span
              aria-hidden
              className="pointer-events-none absolute inset-x-0 top-1/2 h-px bg-display-dim/50"
            />
          ) : null}
          {BANDS.map((band) => (
            <Bar
              key={band}
              band={band}
              lane={lane}
              bind={bind}
              bipolar={bipolar}
            />
          ))}
        </div>
      </div>
      <div className="flex min-w-0 items-center gap-1.5">
        <span className="w-12 shrink-0 text-right text-[9px] leading-none text-muted-foreground">
          Hz
        </span>
        <div className="flex min-w-0 flex-1 gap-px px-px">
          {BANDS.map((band) => {
            const toggle = bind(bandParam(band, "enabled"))
            return (
              <div
                key={band}
                data-param={toggle.info.id}
                className="flex min-w-0 flex-1 flex-col items-center gap-1"
              >
                <span
                  aria-hidden
                  className="truncate font-readout text-[8px] leading-none text-muted-foreground"
                >
                  {shortHz(bandCenter(band))}
                </span>
                <ToggleLed
                  variant="dot"
                  size="sm"
                  aria-label={translatedText(toggle.info.name)}
                  pressed={toggle.value >= 0.5}
                  disabled={toggle.disabled}
                  onPressedChange={(next) => {
                    toggle.onGestureStart()
                    toggle.onValueChange(next ? 1 : 0)
                    toggle.onGestureEnd()
                  }}
                />
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}
