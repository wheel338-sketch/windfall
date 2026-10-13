import { message as i18nMessage } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import { ArrowDown01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import type { Channel } from "@/bindings"
import { ActionButton } from "@/components/action-button"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { useHint } from "@/lib/store/hint"
import { useMixerTrack, useMixerTrackIndex } from "@/lib/store/selectors"
import { colorToCss } from "@/lib/units"

import { RoutingMenu, trackNumber } from "../routing-menu"
import { Section } from "./parts"

/** Which mixer track the channel plays into, with a way to change it and go there. */
export function RoutingSection({ channel }: { channel: Channel }) {
  useLanguageStore((state) => state.language)
  const track = useMixerTrack(channel.mixerTrack)
  const index = useMixerTrackIndex(channel.mixerTrack)
  const hint = useHint(
    t("Mixer track: where this channel's sound goes. Click to route it to another track or a new one")
  )

  return (
    <Section title={t("Mixer track")} className="flex-row items-center">
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button
              variant="outline"
              size="sm"
              aria-label={i18nMessage("Mixer track: {value0}", { value0: track?.name ?? t("missing") })}
              className="min-w-0 flex-1 justify-start gap-2 px-2"
              {...hint}
            />
          }
        >
          <span
            aria-hidden
            className="size-2 shrink-0 rounded-[2px]"
            style={{ backgroundColor: colorToCss(track?.color ?? 0x8b8d98) }}
          />
          <span className="shrink-0 font-readout text-muted-foreground">
            {trackNumber(index)}
          </span>
          <span className="min-w-0 flex-1 truncate text-left">
            {track?.name ?? t("Missing track")}
          </span>
          <HugeiconsIcon
            icon={ArrowDown01Icon}
            strokeWidth={2}
            className="text-muted-foreground"
          />
        </DropdownMenuTrigger>
        <DropdownMenuContent className="w-56">
          <RoutingMenu
            channel={channel.id}
            channelName={channel.name}
            track={channel.mixerTrack}
          />
        </DropdownMenuContent>
      </DropdownMenu>
      <ActionButton
        action="channel.showInMixer"
        variant="ghost"
        size="sm"
        className="shrink-0"
      >{t("Show in mixer")}</ActionButton>
    </Section>
  )
}
