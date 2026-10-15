import { useRef } from "react"
import type { AudioEditPreview } from "./types"
import { clockTime, frameAt, rulerTicks, selectionBetween, type Selection } from "./selection"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"
import { cn } from "@/lib/utils"

export function Waveform({
  preview,
  selection,
  onSelection,
  disabled,
  spectral = false,
}: {
  preview: AudioEditPreview
  selection: Selection
  onSelection(selection: Selection): void
  disabled: boolean
  spectral?: boolean
}) {
  const anchor = useRef<number | null>(null)
  const buckets = preview.peaks.length / 2
  const path = preview.peaks.reduce((path, value, i) => {
    const x = (Math.floor(i / 2) / Math.max(1, buckets - 1)) * 1000
    const y = 80 - Math.max(-1, Math.min(1, value)) * 70
    return `${path}${i % 2 === 0 ? "M" : "L"}${x.toFixed(2)},${y.toFixed(2)} `
  }, "")
  const position = (event: React.PointerEvent<SVGSVGElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect()
    return frameAt(event.clientX, bounds.left, bounds.width, preview.frames)
  }
  const start = Number.isFinite(selection.start)
    ? Math.max(0, Math.min(preview.frames, selection.start))
    : 0
  const end = Number.isFinite(selection.end)
    ? Math.max(start, Math.min(preview.frames, selection.end))
    : start
  const first = (start / preview.frames) * 1000
  const width = ((end - start) / preview.frames) * 1000
  return (
    <div className="flex flex-col gap-1">
      <div className="flex gap-1">
        {spectral && <FrequencyAxis nyquist={preview.sampleRate / 2} />}
        <svg
          role="img"
          aria-label={message("Clip {spectral}, selected frames {start} to {end}", { spectral: spectral ? t("spectrogram") : t("waveform"), start: plainNumber(selection.start), end: plainNumber(selection.end) })}
          viewBox="0 0 1000 160"
          preserveAspectRatio="none"
          className="h-40 min-w-0 flex-1 touch-none rounded-md border bg-background"
          onPointerDown={(event) => {
            if (disabled || event.button !== 0) return
            event.currentTarget.setPointerCapture(event.pointerId)
            anchor.current = position(event)
            onSelection(
              selectionBetween(anchor.current, anchor.current, preview.frames)
            )
          }}
          onPointerMove={(event) => {
            if (disabled || anchor.current === null) return
            onSelection(
              selectionBetween(anchor.current, position(event), preview.frames)
            )
          }}
          onPointerUp={(event) => {
            if (anchor.current === null) return
            onSelection(
              selectionBetween(anchor.current, position(event), preview.frames)
            )
            anchor.current = null
            event.currentTarget.releasePointerCapture(event.pointerId)
          }}
          onPointerCancel={() => {
            anchor.current = null
          }}
          onLostPointerCapture={() => {
            anchor.current = null
          }}
        >
          {spectral ? preview.spectral?.map((value, i) => <rect key={i} x={Math.floor(i/64)*1000/128} y={(63-i%64)*160/64} width={1000/128+0.1} height={160/64+0.1} fill={`hsl(${260-value*210} 85% ${5+value*65}%)`} />) : <>
            <line x1="0" x2="1000" y1="80" y2="80" className="stroke-border" />
            <path d={path} className="stroke-muted-foreground" strokeWidth="1" />
          </>}
          <rect
            x={first}
            y="0"
            width={width}
            height="160"
            className="fill-primary/15 stroke-primary"
          />
        </svg>
      </div>
      <TimeRuler duration={preview.frames / preview.sampleRate} inset={spectral} />
    </div>
  )
}

/** Clip time under the waveform, in minutes:seconds from the clip start. */
function TimeRuler({ duration, inset }: { duration: number; inset: boolean }) {
  return (
    <div aria-hidden className="flex gap-1">
      {inset && <div className="w-14 shrink-0" />}
      <div className="relative h-4 min-w-0 flex-1 overflow-hidden text-[10px] tabular-nums text-muted-foreground">
        {rulerTicks(duration).map(({ seconds, decimals }) => (
          <span
            key={seconds}
            className="absolute top-0 h-full border-l border-border pl-0.5 leading-4"
            style={{ left: `${(seconds / duration) * 100}%` }}
          >
            {/* The last marks sit at the right edge, where a label would be cut. */}
            {seconds / duration <= 0.92 && clockTime(seconds, decimals)}
          </span>
        ))}
      </div>
    </div>
  )
}

/** The spectrogram's linear frequency scale, Nyquist at the top and 0 Hz at the bottom. */
function FrequencyAxis({ nyquist }: { nyquist: number }) {
  return (
    <div aria-hidden className="relative h-40 w-14 shrink-0 text-[10px] tabular-nums text-muted-foreground">
      {[1, 0.75, 0.5, 0.25, 0].map((fraction) => (
        <span
          key={fraction}
          className={cn(
            "absolute right-0",
            fraction === 1 ? "top-0" : fraction === 0 ? "bottom-0" : "-translate-y-1/2"
          )}
          style={fraction === 1 || fraction === 0 ? undefined : { top: `${(1 - fraction) * 100}%` }}
        >
          {frequencyLabel(nyquist * fraction)}
        </span>
      ))}
    </div>
  )
}

/** Hz and kHz are the same in every language; only the decimal mark changes. */
function frequencyLabel(frequency: number): string {
  return frequency >= 1000
    ? `${plainNumber(Math.round(frequency / 100) / 10)} kHz`
    : `${plainNumber(Math.round(frequency))} Hz`
}
