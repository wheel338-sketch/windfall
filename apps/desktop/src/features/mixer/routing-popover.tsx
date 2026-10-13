import { GitForkIcon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { useHint } from "@/lib/store"
import { MASTER_TRACK } from "@/lib/units"
import { cn } from "@/lib/utils"

import { FeedList, useTrackAudioClips, useTrackChannels } from "./channel-chips"
import { OutputSelect } from "./output-select"
import { AddSendMenu, SendList } from "./sends"
import type { StripTrack } from "./strip-track"
import { translatedText as t } from "@/lib/i18n"

function Heading({ children }: { children: string }) {
  return (
    <h3 className="text-[10px] leading-none font-medium text-muted-foreground">
      {children}
    </h3>
  )
}

/** The channels and audio clips of a track, for a strip too low to show them. */
function Channels({ track }: { track: StripTrack }) {
  const channels = useTrackChannels(track.id)
  const clips = useTrackAudioClips(track.id)
  return (
    <section className="flex flex-col gap-1.5">
      <Heading>{t("Plays into it")}</Heading>
      {channels.length > 0 || clips.length > 0 ? (
        <FeedList channels={channels} clips={clips} inputs={0} />
      ) : (
        <p className="text-[10px] text-muted-foreground">
          {t("No channel and no audio clip plays into this track.")}
        </p>
      )}
    </section>
  )
}

type RoutingButtonProps = {
  track: StripTrack
  /** Also list the track's channels, for when the strip has no room to. */
  withChannels: boolean
}

/**
 * The output and sends of a track behind one small button, for panel
 * heights where they do not fit in the strip.
 */
export function RoutingButton({ track, withChannels }: RoutingButtonProps) {
  const master = track.id === MASTER_TRACK
  const rerouted = !master && track.output !== MASTER_TRACK
  const marked = rerouted || track.sends.length > 0
  const hint = useHint(
    master
      ? t("Channels that play straight into the master")
      : t("Output and sends of this track")
  )

  return (
    <Popover>
      <PopoverTrigger
        render={
          <button
            type="button"
            data-slot="track-routing"
            aria-label={master ? t("Channels") : t("Routing")}
            className={cn(
              "relative flex size-4 shrink-0 items-center justify-center rounded-sm text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring aria-expanded:bg-muted aria-expanded:text-foreground",
              marked && "text-foreground"
            )}
            {...hint}
          />
        }
      >
        <HugeiconsIcon icon={GitForkIcon} strokeWidth={2} className="size-3" />
        {marked && (
          <span
            aria-hidden
            className="absolute top-0 right-0 size-1 rounded-full bg-brand"
          />
        )}
      </PopoverTrigger>
      <PopoverContent side="bottom" align="start" className="w-48 gap-3 p-2.5">
        {withChannels && <Channels track={track} />}
        {!master && (
          <>
            <section className="flex flex-col gap-1.5">
              <Heading>{t("Output")}</Heading>
              <OutputSelect id={track.id} output={track.output} />
            </section>
            <section className="flex flex-col gap-1">
              <Heading>{t("Sends")}</Heading>
              <SendList
                id={track.id}
                sends={track.sends}
                className="max-h-40"
              />
              <AddSendMenu id={track.id} className="self-start" />
            </section>
          </>
        )}
      </PopoverContent>
    </Popover>
  )
}
