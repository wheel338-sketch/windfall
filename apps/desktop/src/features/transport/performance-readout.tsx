import { useRef } from "react"

import { useHint } from "@/lib/store/hint"
import { useRealtime } from "@/lib/store/realtime"
import { MemoryReadout } from "./memory-readout"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

/** Text this small is unreadable if it changes every frame. */
const REFRESH_MS = 250

/**
 * How hard the audio engine is working, and how many buffers arrived late.
 * Late buffers are heard as clicks, so they turn the readout amber.
 */
export function PerformanceReadout() {
  const cpu = useRef<HTMLSpanElement>(null)
  const late = useRef<HTMLSpanElement>(null)
  const lastDraw = useRef(0)
  const peak = useRef(0)
  const hint = useHint(
    t("Engine load: the share of each audio buffer's time spent computing it")
  )

  useRealtime((frame) => {
    peak.current = Math.max(peak.current, frame.cpu)
    const now = performance.now()
    if (now - lastDraw.current < REFRESH_MS) return
    lastDraw.current = now
    if (cpu.current) {
      cpu.current.textContent = `${Math.round(peak.current * 100)}%`
      cpu.current.dataset.high = String(peak.current > 0.8)
    }
    if (late.current) {
      late.current.textContent =
        frame.xruns === 1 ? t("1 dropout") : message("{xruns} dropouts", { xruns: plainNumber(frame.xruns) })
      late.current.hidden = frame.xruns === 0
    }
    peak.current = 0
  })

  return (
    <div
      className="flex items-center gap-2 text-[0.6875rem] text-muted-foreground"
      {...hint}
    >
      <span className="flex items-baseline gap-1">
        CPU
        <span
          ref={cpu}
          className="w-7 text-right font-readout text-foreground data-[high=true]:text-warn"
        >
          0%
        </span>
      </span>
      <span ref={late} hidden className="text-warn" />
      <MemoryReadout />
    </div>
  )
}
