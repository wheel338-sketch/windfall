import { useEffect, useRef } from "react"

import {
  gainToFaderPosition,
  LevelMeter,
  type LevelMeterHandle,
} from "@/components/audio"
import { useHint } from "@/lib/store/hint"
import { meterFeed } from "@/lib/store/realtime"
import { onProjectReplaced } from "@/lib/store/replaced"
import { MASTER_TRACK } from "@/lib/units"
import { translatedText as t } from "@/lib/i18n"

// Made once, so the meter subscribes once and not on every render.
const MASTER_FEED = meterFeed(MASTER_TRACK)

/**
 * The master level in the transport bar: left above right, on the same dB
 * scale as the mixer's faders, with a clip light at the loud end that stays
 * lit until it is clicked.
 */
export function MasterMeterSlot() {
  const hint = useHint(t("Master level. Click to clear the clip light"))
  const meter = useRef<LevelMeterHandle>(null)

  // A clip in the song before is not a clip in this one.
  useEffect(() => onProjectReplaced(() => meter.current?.reset()), [])

  return (
    <div
      data-slot="master-meter"
      role="group"
      aria-label={t("Master level")}
      className="hidden w-24 shrink-0 items-center lg:flex"
      {...hint}
    >
      <LevelMeter
        ref={meter}
        orientation="horizontal"
        subscribe={MASTER_FEED}
        taper={gainToFaderPosition}
        className="h-2.5 w-full"
      />
    </div>
  )
}
