import { memo } from "react"

import type { ChannelId, TrackId } from "@/bindings"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { useHint } from "@/lib/store/hint"
import { useMixerTrack, useMixerTrackIndex } from "@/lib/store/selectors"
import { useUiStore } from "@/lib/store/ui"
import { colorToCss } from "@/lib/units"

import { selectChannel } from "./channel-ops"
import { RoutingMenu, trackNumber } from "./routing-menu"
import { translatedText as t, message } from "@/lib/i18n"

type MixerBadgeProps = {
  channel: ChannelId
  channelName: string
  track: TrackId
}

/**
 * Shows which mixer track a channel plays into, in that track's color.
 * Clicking it selects the track in the mixer and opens the routing menu.
 */
export const MixerBadge = memo(function MixerBadge({
  channel,
  channelName,
  track: trackId,
}: MixerBadgeProps) {
  const track = useMixerTrack(trackId)
  const index = useMixerTrackIndex(trackId)
  const trackName = track?.name ?? t("a missing track")
  const hint = useHint(
    message("{channelName} plays into mixer track {trackNumber}, {trackName}. Click to select it or route somewhere else", { channelName, trackNumber: trackNumber(index), trackName })
  )

  return (
    <DropdownMenu
      onOpenChange={(open) => {
        if (!open) return
        selectChannel(channel)
        useUiStore.getState().selectTrack(trackId)
      }}
    >
      <DropdownMenuTrigger
        render={
          <button
            type="button"
            aria-label={message("{channelName} plays into {trackName}. Change routing", { channelName, trackName })}
            className="flex h-5 w-full items-center gap-1 rounded-[3px] bg-(--wf-step-off) pr-1 font-readout text-[0.625rem] text-foreground/85 outline-none hover:bg-(--wf-step-off-alt) hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring aria-expanded:bg-(--wf-step-off-alt)"
            {...hint}
          />
        }
      >
        <span
          aria-hidden
          className="h-full w-1 shrink-0 rounded-l-[3px]"
          style={{ backgroundColor: colorToCss(track?.color ?? 0x8b8d98) }}
        />
        <span aria-hidden className="text-muted-foreground">
          →
        </span>
        <span className="min-w-0 flex-1 truncate text-right">
          {trackNumber(index)}
        </span>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-56">
        <RoutingMenu
          channel={channel}
          channelName={channelName}
          track={trackId}
        />
      </DropdownMenuContent>
    </DropdownMenu>
  )
})
