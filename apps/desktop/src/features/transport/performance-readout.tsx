import { useRef, useState } from "react"

import { Button } from "@/components/ui/button"
import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover"
import { runAction } from "@/lib/actions"
import { useEngineStore } from "@/lib/store/engine"
import { useHint } from "@/lib/store/hint"
import { useRealtime } from "@/lib/store/realtime"
import { MemoryReadout } from "./memory-readout"
import { translatedText as t, formatDate, message, plainNumber } from "@/lib/i18n"

/** Text this small is unreadable if it changes every frame. */
const REFRESH_MS = 250

/** What is known about the dropouts the readout is showing. */
export interface DropoutLog {
  /** The engine's count when the readout was last reset. */
  baseline: number
  /** The engine's count in the newest frame. */
  seen: number
  /** When the newest dropout was noticed, as `Date.now()`. */
  lastAt: number | null
  /** Whether the song or pattern was playing when it was noticed. */
  lastPlaying: boolean
  /** Engine load at that moment, 0 to 1. */
  lastCpu: number
}

export function emptyDropoutLog(): DropoutLog {
  return { baseline: 0, seen: 0, lastAt: null, lastPlaying: false, lastCpu: 0 }
}

/**
 * Takes in the engine's cumulative count. The engine starts counting again
 * when the audio output is reopened, so a count that goes down starts the
 * log again as well.
 */
export function noteDropouts(
  log: DropoutLog,
  frame: { xruns: number; playing: boolean; cpu: number },
  now: number
): DropoutLog {
  if (frame.xruns === log.seen) return log
  if (frame.xruns < log.seen) return { ...emptyDropoutLog(), seen: frame.xruns }
  return {
    ...log,
    seen: frame.xruns,
    lastAt: now,
    lastPlaying: frame.playing,
    lastCpu: frame.cpu,
  }
}

/** Dropouts shown: those since the last reset. */
export function shownDropouts(log: DropoutLog): number {
  return Math.max(0, log.seen - log.baseline)
}

/**
 * How hard the audio engine is working, and how many buffers arrived late.
 * Late buffers are heard as clicks, so they turn the readout amber; the
 * count opens what is known about them and what to do.
 */
export function PerformanceReadout() {
  const cpu = useRef<HTMLSpanElement>(null)
  const late = useRef<HTMLButtonElement>(null)
  const lastDraw = useRef(0)
  const peak = useRef(0)
  const log = useRef(emptyDropoutLog())
  const [open, setOpen] = useState(false)
  // What the open details show; refreshed when another dropout comes in.
  const [details, setDetails] = useState(emptyDropoutLog)
  const hint = useHint(
    t("Engine load: the share of each audio buffer's time spent computing it")
  )
  const lateHint = useHint(t("Audio dropouts: buffers that were not ready in time. Click for details"))

  const drawLate = () => {
    const count = shownDropouts(log.current)
    if (!late.current) return
    late.current.textContent =
      count === 1 ? t("1 dropout") : message("{xruns} dropouts", { xruns: plainNumber(count) })
    // Kept while its details are open, so the popup has an anchor.
    late.current.hidden = count === 0 && !open
  }

  useRealtime((frame) => {
    const next = noteDropouts(log.current, frame, Date.now())
    if (next !== log.current) {
      log.current = next
      if (open) setDetails(next)
    }
    peak.current = Math.max(peak.current, frame.cpu)
    const now = performance.now()
    if (now - lastDraw.current < REFRESH_MS) return
    lastDraw.current = now
    if (cpu.current) {
      cpu.current.textContent = `${Math.round(peak.current * 100)}%`
      cpu.current.dataset.high = String(peak.current > 0.8)
    }
    drawLate()
    peak.current = 0
  })

  const reset = () => {
    log.current = { ...log.current, baseline: log.current.seen, lastAt: null }
    setDetails(log.current)
    setOpen(false)
    drawLate()
    if (late.current) late.current.hidden = true
  }

  return (
    <div className="flex items-center gap-2 text-[0.6875rem] text-muted-foreground">
      <span className="flex items-baseline gap-1" {...hint}>
        CPU
        <span
          ref={cpu}
          className="w-7 text-right font-readout text-foreground data-[high=true]:text-warn"
        >
          0%
        </span>
      </span>
      <Popover
        open={open}
        onOpenChange={(next) => {
          if (next) setDetails(log.current)
          setOpen(next)
        }}
      >
        <PopoverTrigger
          render={
            <button
              ref={late}
              type="button"
              hidden
              className="rounded-sm text-warn underline decoration-dotted underline-offset-2 outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
              {...lateHint}
            />
          }
        />
        <PopoverContent align="end" className="w-80 gap-3">
          <DropoutDetails log={details} onReset={reset} />
        </PopoverContent>
      </Popover>
      <MemoryReadout />
    </div>
  )
}

function DropoutDetails({ log, onReset }: { log: DropoutLog; onReset: () => void }) {
  const status = useEngineStore((state) => state.status)
  const count = shownDropouts(log)
  return (
    <>
      <PopoverHeader>
        <PopoverTitle className="text-xs font-medium">{t("Audio dropouts")}</PopoverTitle>
        <PopoverDescription>
          {t("A dropout is an audio buffer that was not ready in time. While something is sounding it can be heard as a click; while nothing is sounding it is silent.")}
        </PopoverDescription>
      </PopoverHeader>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
        <dt className="text-muted-foreground">{t("Counted")}</dt>
        <dd className="font-readout">{plainNumber(count)}</dd>
        {log.lastAt !== null && (
          <>
            <dt className="text-muted-foreground">{t("Last dropout")}</dt>
            <dd>
              {message(log.lastPlaying ? "{time}, while playing" : "{time}, while stopped", {
                time: formatDate(log.lastAt, { timeStyle: "medium" }),
              })}
            </dd>
            <dt className="text-muted-foreground">{t("Engine load then")}</dt>
            <dd className="font-readout">{`${Math.round(log.lastCpu * 100)}%`}</dd>
          </>
        )}
        {status?.running && (
          <>
            <dt className="text-muted-foreground">{t("Output buffer")}</dt>
            <dd className="font-readout">
              {message("{buffer} samples, {latency} ms", {
                buffer: plainNumber(status.bufferFrames),
                latency: status.latencyMs.toFixed(1),
              })}
            </dd>
          </>
        )}
      </dl>
      <p className="text-muted-foreground">
        {t("If dropouts keep coming while you play, raise the buffer size in the audio settings. A few while editing with nothing sounding usually mean the system held up the audio thread for a moment; they do not affect exports.")}
      </p>
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onReset}>
          {t("Reset count")}
        </Button>
        <Button size="sm" variant="outline" onClick={() => void runAction("options.settings")}>
          {t("Audio settings…")}
        </Button>
      </div>
    </>
  )
}
