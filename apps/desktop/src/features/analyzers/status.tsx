import { cn } from "@/lib/utils"

import { useAnalyzerStatus } from "./feed"
import type { AnalyzerReason, AnalyzerStatus } from "./types"
import { formatNumber, translatedText as t } from "@/lib/i18n"

const REASONS: Record<AnalyzerReason, string> = {
  get desktopRequired() { return t("Needs the desktop app's audio engine") },
  get notSubscribed() { return t("Not connected to the analyzer") },
  get invalidSlot() { return t("Not connected to the analyzer") },
  get subscriptionCapacity() { return t("Four analyzer views are open already") },
  get sourceCapacity() { return t("Two other sources are being analyzed already") },
  get staleProject() { return t("The project changed") },
  get sourceMissing() { return t("This track is no longer in the project") },
  get effectMissing() { return t("This effect is no longer in the project") },
  get effectNotOnSource() { return t("This effect is not on the selected track") },
  get rateUnsupported() { return t("The audio device's sample rate is not supported") },
  get latencyUnsupported() { return t("This source's latency is more than a second") },
  get preparation() { return t("The analyzer could not be started") },
  get exhausted() { return t("The analyzer needs the app to be restarted") },
  get audioStalled() { return t("No audio is arriving") },
  get viewStalled() { return t("This view fell behind") },
  get projectReplaced() { return t("The project was replaced") },
}

/**
 * What a slot's standing means, in a few words. Only "Live" says that what
 * is drawn is current audio.
 */
export function analyzerStatusText(status: AnalyzerStatus | null): string {
  if (!status) return t("No source selected")
  switch (status.phase) {
    case "idle":
      return t("No source selected")
    case "preparing":
      return t("Starting…")
    case "awaitingAudio":
      return t("Waiting for audio")
    case "live":
      return t("Live")
    case "waitingForStream":
      return t("No audio device is open")
    case "stalled":
      return status.reason ? REASONS[status.reason] : t("Stalled")
    case "unavailable":
      return REASONS[status.reason ?? "desktopRequired"]
    case "refused":
      return status.reason ? REASONS[status.reason] : t("Not available")
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
        {live && rate ? ` · ${formatNumber(rate / 1000, { minimumFractionDigits: 1, maximumFractionDigits: 1, useGrouping: false })} kHz` : ""}
      </span>
    </p>
  )
}
