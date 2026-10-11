import { useCallback, useEffect, useRef } from "react"

import { LevelMeter, type LevelMeterHandle } from "@/components/audio"
import { meterFeed } from "@/lib/store/realtime"
import { onProjectReplaced } from "@/lib/store/replaced"
import { gainToDb, MASTER_TRACK } from "@/lib/units"

const MASTER_FEED = meterFeed(MASTER_TRACK)

function setLevel(element: HTMLElement | null, gain: number) {
  if (!element || Number.isNaN(gain)) return
  const db = gainToDb(Math.abs(gain))
  const text = db === -Infinity ? "−∞ dBFS" : `${db.toFixed(1)} dBFS`
  if (element.textContent !== text) element.textContent = text
}

/** Stereo master peaks from the existing feed, with LevelMeter's clip latch. */
export function LargeMasterMeter() {
  const meter = useRef<LevelMeterHandle>(null)
  const left = useRef<HTMLSpanElement>(null)
  const right = useRef<HTMLSpanElement>(null)
  // One subscription supplies both the bars and the numeric peak readouts.
  const subscribe = useCallback(
    (listener: (left: number, right?: number) => void) =>
      MASTER_FEED((leftGain, rightGain) => {
        listener(leftGain, rightGain)
        setLevel(left.current, leftGain)
        setLevel(right.current, rightGain)
      }),
    []
  )

  useEffect(
    () =>
      onProjectReplaced(() => {
        meter.current?.reset()
        setLevel(left.current, 0)
        setLevel(right.current, 0)
      }),
    []
  )

  return (
    <div
      role="group"
      aria-label="Master level"
      data-slot="large-master-meter"
      className="flex flex-col items-center gap-4 rounded-lg bg-display p-6"
    >
      <div className="grid w-full grid-cols-2 gap-4 text-center font-readout tabular-nums">
        <div
          role="group"
          aria-label="Left peak level"
          className="flex flex-col gap-1"
        >
          <span className="text-xs text-display-dim">Left</span>
          <span ref={left} data-side="left" className="text-xl text-display-foreground">
            −∞ dBFS
          </span>
        </div>
        <div
          role="group"
          aria-label="Right peak level"
          className="flex flex-col gap-1"
        >
          <span className="text-xs text-display-dim">Right</span>
          <span ref={right} data-side="right" className="text-xl text-display-foreground">
            −∞ dBFS
          </span>
        </div>
      </div>
      <LevelMeter
        ref={meter}
        channels={2}
        orientation="vertical"
        subscribe={subscribe}
        minDb={-60}
        maxDb={6}
        clipLabel="Clear master clip indicator"
        aria-label="Stereo master meter"
        className="h-[min(24rem,50vh)] w-36"
      />
      <span className="text-xs text-display-dim">
        Peak level · −60 to +6 dBFS
      </span>
    </div>
  )
}
