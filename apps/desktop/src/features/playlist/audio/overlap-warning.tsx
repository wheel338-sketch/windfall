import { Alert02Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { useMemo } from "react"

import { audioClipOverlap } from "@/lib/audio-clips"
import { useHint } from "@/lib/store/hint"
import { useProjectStore } from "@/lib/store/project"
import { tickToPosition } from "@/lib/time"
import { MAX_AUDIO_CLIPS } from "@/lib/units"

import type { GridMetrics } from "../metrics"
import { seekSong } from "../ops"
import { message, translatedText as t } from "@/lib/i18n"

/** Where audio clips will be silent, and what to say about it. */
export type SilentClips = {
  /** The first place in the song where it happens. */
  tick: number
  bar: number
  text: string
}

/**
 * Whether the song has more audio clips sounding together than the engine
 * plays, and where first. Null while it does not.
 *
 * This is worked out from where the clips lie. The engine is to report how
 * many clips it really could not start; when it does, that count belongs
 * here in place of the estimate, and nothing that shows the warning has to
 * change.
 */
export function useSilentClips(): SilentClips | null {
  const playlist = useProjectStore((state) => state.project.playlist)
  const signature = useProjectStore(
    (state) => state.project.settings.timeSignature
  )
  return useMemo(() => {
    const { overAt } = audioClipOverlap(playlist)
    if (overAt === null) return null
    const { bar } = tickToPosition(overAt, signature)
    return {
      tick: overAt,
      bar,
      text: `More than ${MAX_AUDIO_CLIPS} audio clips overlap at bar ${bar}; the extra ones will be silent`,
    }
  }, [playlist, signature])
}

/** How far into the view the place the warning jumps to lands. */
const LEAD = 0.2

function Warning({
  silent,
  metrics,
}: {
  silent: SilentClips
  metrics: GridMetrics
}) {
  const hint = useHint(
    message("{silentText}. Click to go there. Mute or move some of them to hear the rest", { silentText: silent.text })
  )

  function show() {
    const viewport = metrics.viewport
    const visible = viewport.width / viewport.pxPerTick
    metrics.setViewport({
      ...viewport,
      scrollTick: Math.max(0, silent.tick - visible * LEAD),
    })
    // The song position goes there too, so Play starts on the spot.
    void seekSong(silent.tick)
  }

  return (
    <button
      type="button"
      data-slot="playlist-overlap-warning"
      aria-label={message("{silentText}. Go there", { silentText: silent.text })}
      title={silent.text}
      className="flex h-6 shrink-0 items-center gap-1 rounded-md bg-warn/15 px-1.5 text-warn outline-none hover:bg-warn/25 focus-visible:ring-2 focus-visible:ring-ring"
      onClick={show}
      {...hint}
    >
      <HugeiconsIcon icon={Alert02Icon} strokeWidth={2} className="size-3.5" />
      <span>
        {t("Over")}{" "}{MAX_AUDIO_CLIPS}{" "}{t("clips at bar")}{" "}{silent.bar}
      </span>
    </button>
  )
}

/**
 * A warning in the playlist's toolbar while more audio clips overlap than
 * can sound. Clicking it goes to the first place where they do.
 */
export function OverlapWarning({ metrics }: { metrics: GridMetrics }) {
  const silent = useSilentClips()
  return silent ? <Warning silent={silent} metrics={metrics} /> : null
}
