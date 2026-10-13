import { useEffect, useEffectEvent, useRef } from "react"

import type { EffectId } from "@/bindings"
import { gainReductionFeed } from "@/lib/store"
import { cn } from "@/lib/utils"
import { message, translatedText as t } from "@/lib/i18n"

/*
 * Gain reduction of a compressor or limiter, drawn from the realtime feed.
 * Nothing here is React state: a new reading sets a transform and a line of
 * text, so a meter costs no renders however fast readings arrive.
 */

/** The deepest reduction a meter shows, in dB. */
export const REDUCTION_RANGE_DB = 24
/** How fast a reading falls back once the reduction lets go. */
const RELEASE_DB_PER_SECOND = 48
/** How long the deepest reading is held before it falls. */
const PEAK_HOLD_MS = 1200

export type ReductionState = {
  /** The bar: follows the readings up at once and falls back at a rate. */
  level: number
  /** The deepest reading lately. */
  peak: number
  peakAt: number
}

export function createReductionState(): ReductionState {
  return { level: 0, peak: 0, peakAt: 0 }
}

/** Moves a meter on by one reading taken at `now` milliseconds. */
export function advanceReduction(
  state: ReductionState,
  db: number,
  now: number,
  elapsedMs: number
): ReductionState {
  const reading = Number.isFinite(db) ? Math.max(0, db) : 0
  const fall = (RELEASE_DB_PER_SECOND * Math.max(0, elapsedMs)) / 1000
  state.level = Math.max(reading, state.level - fall)
  if (reading >= state.peak) {
    state.peak = reading
    state.peakAt = now
  } else if (now - state.peakAt > PEAK_HOLD_MS) {
    state.peak = Math.max(state.level, state.peak - fall)
  }
  return state
}

/** A reduction in dB as its share of the meter's travel, 0 to 1. */
export function reductionPosition(db: number, rangeDb = REDUCTION_RANGE_DB) {
  return Math.min(1, Math.max(0, db / rangeDb))
}

/** "−4.2" for a reduction of 4.2 dB, and "0.0" for none. */
export function formatReduction(db: number): string {
  const rounded = Math.round(db * 10) / 10
  return rounded <= 0 ? "0.0" : `−${rounded.toFixed(1)}`
}

/**
 * Feeds the readings of one effect through the meter's fall-back and hands
 * the result to `draw`, which runs only when something changed.
 */
function useReduction(
  effect: EffectId,
  draw: (state: Readonly<ReductionState>) => void
) {
  const onDraw = useEffectEvent(draw)
  useEffect(() => {
    const state = createReductionState()
    let last = performance.now()
    let shownLevel = -1
    let shownPeak = -1
    return gainReductionFeed(effect)((db) => {
      const now = performance.now()
      advanceReduction(state, db, now, now - last)
      last = now
      if (state.level === shownLevel && state.peak === shownPeak) return
      shownLevel = state.level
      shownPeak = state.peak
      onDraw(state)
    })
  }, [effect])
}

type BarProps = { effect: EffectId; className?: string }

/**
 * The sliver under an effect's name on a strip: it grows from the left
 * while the effect turns the signal down, so a compressor at work is seen
 * without opening it. Small reductions are stretched to stay visible.
 */
export function GainReductionBar({ effect, className }: BarProps) {
  const fill = useRef<HTMLSpanElement>(null)
  useReduction(effect, (state) => {
    if (!fill.current) return
    const share = reductionPosition(state.level) ** 0.6
    fill.current.style.transform = `scaleX(${share.toFixed(3)})`
  })
  return (
    <span
      aria-hidden
      data-slot="gain-reduction-bar"
      className={cn(
        "pointer-events-none block h-0.5 overflow-hidden rounded-full bg-(--wf-meter-bg)",
        className
      )}
    >
      <span
        ref={fill}
        className="block size-full origin-left bg-(--wf-meter-mid)"
        style={{ transform: "scaleX(0)" }}
      />
    </span>
  )
}

const MARKS = [0, 6, 12, 18, 24]

type MeterProps = {
  effect: EffectId
  /** Says what is being turned down, such as "Compressor". */
  label: string
  className?: string
}

/**
 * The gain reduction meter of an effect's editor: a bar that falls from the
 * top, a line that holds the deepest reduction, and that figure in dB.
 */
export function GainReductionMeter({ effect, label, className }: MeterProps) {
  const fill = useRef<HTMLDivElement>(null)
  const hold = useRef<HTMLDivElement>(null)
  const text = useRef<HTMLSpanElement>(null)

  useReduction(effect, (state) => {
    if (fill.current) {
      fill.current.style.transform = `scaleY(${reductionPosition(state.level).toFixed(4)})`
    }
    if (hold.current) {
      const at = reductionPosition(state.peak)
      hold.current.style.top = `${(at * 100).toFixed(2)}%`
      hold.current.style.opacity = state.peak > 0.05 ? "1" : "0"
    }
    if (text.current) text.current.textContent = formatReduction(state.peak)
  })

  return (
    <div
      role="group"
      aria-label={message("{label} gain reduction", { label })}
      data-slot="gain-reduction-meter"
      className={cn(
        "flex w-11 shrink-0 flex-col items-center gap-1",
        className
      )}
    >
      <div className="flex min-h-0 w-full flex-1 justify-center gap-1">
        <div className="relative w-2.5 overflow-hidden rounded-[2px] bg-(--wf-meter-bg) shadow-[inset_0_1px_1px_rgb(0_0_0/0.35)]">
          <div
            ref={fill}
            data-slot="gain-reduction-fill"
            className="absolute inset-0 origin-top bg-(--wf-meter-mid)"
            style={{ transform: "scaleY(0)" }}
          />
          <div
            ref={hold}
            className="absolute inset-x-0 h-px bg-foreground"
            style={{ top: 0, opacity: 0 }}
          />
        </div>
        <div aria-hidden className="relative w-4">
          {MARKS.map((mark) => (
            <span
              key={mark}
              className="absolute left-0 -translate-y-1/2 font-readout text-[8px] leading-none text-muted-foreground first:translate-y-0 last:-translate-y-full"
              style={{ top: `${reductionPosition(mark) * 100}%` }}
            >
              {mark}
            </span>
          ))}
        </div>
      </div>
      <div className="flex h-3.5 w-full items-center justify-center rounded-[3px] bg-display font-readout text-[9px] leading-none text-display-foreground shadow-[inset_0_1px_1px_rgb(0_0_0/0.4)]">
        <span className="sr-only">{t("Deepest reduction")}{" "}</span>
        <span ref={text} data-slot="gain-reduction-value">
          0.0
        </span>
        <span className="sr-only"> dB</span>
      </div>
    </div>
  )
}
