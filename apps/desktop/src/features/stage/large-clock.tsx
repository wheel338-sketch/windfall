import { useRef } from "react"

import { songTempoMap } from "@/lib/automation/tempo-map"
import { useProjectStore } from "@/lib/store/project"
import { useRealtime } from "@/lib/store/realtime"
import { useTransportStore } from "@/lib/store/transport"
import { formatMusicalPosition } from "@/lib/timeline"
import { formatClock, ticksToSeconds } from "@/lib/time"

function setText(element: HTMLElement | null, text: string) {
  if (element && element.textContent !== text) element.textContent = text
}

/** The transport's position at stage size, without React updates per frame. */
export function LargeClock() {
  const position = useRef<HTMLSpanElement>(null)
  const clock = useRef<HTMLSpanElement>(null)

  useRealtime((frame) => {
    const project = useProjectStore.getState().project
    const transport = useTransportStore.getState()
    const inSong = transport.mode === "song"
    const pattern = inSong
      ? null
      : project.patterns.find((item) => item.id === transport.pattern)
    const signature = pattern?.timeSignature ?? project.settings.timeSignature
    const meters =
      (inSong ? project.playlist.timeline : pattern?.timeline)?.meters ?? []
    setText(
      position.current,
      formatMusicalPosition(frame.tick, signature, meters)
    )
    const seconds = inSong
      ? songTempoMap(project).secondsAt(frame.tick)
      : ticksToSeconds(frame.tick, project.settings.tempoBpm)
    setText(clock.current, formatClock(seconds))
  })

  return (
    <div
      role="group"
      aria-label="Song position"
      data-slot="large-clock"
      className="flex min-w-0 flex-col items-center gap-6 rounded-lg bg-display px-4 py-8 font-readout tabular-nums"
    >
      <div className="flex flex-col items-center gap-2">
        <span className="text-xs text-display-dim">Bar, beat and step</span>
        <span
          ref={position}
          data-part="position"
          title="Bar, beat and step"
          className="text-[clamp(2rem,8vw,5rem)] leading-none font-medium text-display-foreground"
        >
          001:01:1
        </span>
      </div>
      <div className="flex flex-col items-center gap-2">
        <span className="text-xs text-display-dim">Minutes and seconds</span>
        <span
          ref={clock}
          data-part="time"
          title="Minutes and seconds"
          className="text-[clamp(1.5rem,6vw,3.5rem)] leading-none text-display-foreground"
        >
          0:00.00
        </span>
      </div>
    </div>
  )
}
