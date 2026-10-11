import { cn } from "@/lib/utils"

import { useAnalyzerStatus } from "./feed"
import type { AnalyzerReason, AnalyzerStatus } from "./types"

const REASONS: Record<AnalyzerReason, string> = {
  desktopRequired: "Needs the desktop app's audio engine",
  notSubscribed: "Not connected to the analyzer",
  invalidSlot: "Not connected to the analyzer",
  subscriptionCapacity: "Four analyzer views are open already",
  sourceCapacity: "Two other sources are being analyzed already",
  staleProject: "The project changed",
  sourceMissing: "This track is no longer in the project",
  effectMissing: "This effect is no longer in the project",
  effectNotOnSource: "This effect is not on the selected track",
  rateUnsupported: "The audio device's sample rate is not supported",
  latencyUnsupported: "This source's latency is more than a second",
  preparation: "The analyzer could not be started",
  exhausted: "The analyzer needs the app to be restarted",
  audioStalled: "No audio is arriving",
  viewStalled: "This view fell behind",
  projectReplaced: "The project was replaced",
}

/**
 * What a slot's standing means, in a few words. Only "Live" says that what
 * is drawn is current audio.
 */
export function analyzerStatusText(status: AnalyzerStatus | null): string {
  if (!status) return "No source selected"
  switch (status.phase) {
    case "idle":
      return "No source selected"
    case "preparing":
      return "Starting…"
    case "awaitingAudio":
      return "Waiting for audio"
    case "live":
      return "Live"
    case "waitingForStream":
      return "No audio device is open"
    case "stalled":
      return status.reason ? REASONS[status.reason] : "Stalled"
    case "unavailable":
      return REASONS[status.reason ?? "desktopRequired"]
    case "refused":
      return status.reason ? REASONS[status.reason] : "Not available"
  }
}

type StatusProps = {
  slot: string
  /** Says where the audio is taken from, such as "Track output". */
  location?: string
  className?: string
}

/**
 * One line under an analyzer view: whether it is live, and if not, why.
 * It is a status region, so a change is announced without taking focus.
 */
export function AnalyzerStatusLine({ slot, location, className }: StatusProps) {
  const status = useAnalyzerStatus(slot)
  const live = status?.phase === "live"
  const rate = status?.sampleRate
  return (
    <p
      role="status"
      data-slot="analyzer-status"
      data-phase={status?.phase ?? "idle"}
      data-reason={status?.reason ?? undefined}
      data-source={status?.source ? JSON.stringify(status.source) : undefined}
      data-effect={status?.effect ?? undefined}
      className={cn(
        "flex min-w-0 items-center gap-1.5 text-[10px] leading-snug text-muted-foreground",
        className
      )}
    >
      <span
        aria-hidden
        className={cn(
          "size-1.5 shrink-0 rounded-full",
          live ? "bg-(--wf-meter-low)" : "bg-muted-foreground/50"
        )}
      />
      <span className="truncate">
        {analyzerStatusText(status)}
        {location ? ` · ${location}` : ""}
        {live && rate ? ` · ${(rate / 1000).toFixed(1)} kHz` : ""}
      </span>
    </p>
  )
}
