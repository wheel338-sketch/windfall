import type { CSSProperties } from "react"
import { useShallow } from "zustand/react/shallow"

import type { Channel, ClipId, TrackId } from "@/bindings"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { revealClips } from "@/features/playlist/reveal"
import { useHint, useProjectStore, useUiStore } from "@/lib/store"
import { colorToCss } from "@/lib/units"
import { cn } from "@/lib/utils"

import { audioClipsOfTrack, channelsOfTrack, trackInputCount } from "./routing"
import { message, translatedText as t, plainNumber } from "@/lib/i18n"

/** The channels that play into a track, in rack order. */
export function useTrackChannels(id: TrackId): Channel[] {
  return useProjectStore(
    useShallow((state) => channelsOfTrack(state.project.channels, id))
  )
}

/** The audio clips on the playlist that play into a track. */
export function useTrackAudioClips(id: TrackId): readonly ClipId[] {
  return useProjectStore((state) =>
    audioClipsOfTrack(state.project.playlist.clips, id)
  )
}

/**
 * One channel that plays into the track, in the channel's color. Clicking
 * it selects the channel, which the channel rack highlights.
 */
export function ChannelChip({
  channel,
  className,
}: {
  channel: Channel
  className?: string
}) {
  const selected = useUiStore((state) => state.selectedChannel === channel.id)
  const hint = useHint(
    message("The channel \"{channelName}\" plays into this track. Click to select it in the channel rack", { channelName: channel.name })
  )
  return (
    <button
      type="button"
      data-slot="channel-chip"
      aria-pressed={selected}
      aria-label={message("Channel {channelName}", { channelName: channel.name })}
      title={channel.name}
      style={{ "--chip": colorToCss(channel.color) } as CSSProperties}
      className={cn(
        "flex h-4 min-w-0 items-center gap-1 rounded-[3px] bg-[color-mix(in_oklch,var(--chip)_16%,transparent)] px-1 text-[10px] leading-none text-[color-mix(in_oklch,var(--chip)_45%,var(--foreground))] outline-none hover:bg-[color-mix(in_oklch,var(--chip)_28%,transparent)] focus-visible:outline-2 focus-visible:outline-ring focus-visible:outline-solid aria-pressed:bg-[color-mix(in_oklch,var(--chip)_38%,transparent)] aria-pressed:text-foreground aria-pressed:ring-1 aria-pressed:ring-(--chip)",
        className
      )}
      onClick={() => useUiStore.getState().selectChannel(channel.id)}
      {...hint}
    >
      <span
        aria-hidden
        className="size-1.5 shrink-0 rounded-full bg-(--chip)"
      />
      <span className="truncate">{channel.name}</span>
    </button>
  )
}

const clipsLabel = (count: number) =>
  count === 1 ? t("1 audio clip") : message("{count} audio clips", { count: plainNumber(count) })

const tracksLabel = (count: number) =>
  count === 1 ? t("1 track in") : message("{count} tracks in", { count: plainNumber(count) })

/**
 * The audio clips that play into the track, as a count. Clicking it selects
 * them on the playlist and brings it forward.
 */
export function AudioClipsChip({
  clips,
  className,
}: {
  clips: readonly ClipId[]
  className?: string
}) {
  const label = clipsLabel(clips.length)
  const hint = useHint(
    clips.length === 1
      ? t("An audio clip on the playlist plays into this track. Click to select it there")
      : message("{count} audio clips on the playlist play into this track. Click to select them there", { count: plainNumber(clips.length) })
  )
  return (
    <button
      type="button"
      data-slot="track-audio-clips"
      aria-label={message("{label} of this track", { label })}
      className={cn(
        "flex h-4 min-w-0 items-center rounded-[3px] bg-muted px-1 text-[10px] leading-none text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring focus-visible:outline-solid",
        className
      )}
      onClick={() => revealClips(clips, { focus: true })}
      {...hint}
    >
      <span className="truncate">{label}</span>
    </button>
  )
}

function FedByTracks({
  count,
  className,
}: {
  count: number
  className?: string
}) {
  const hint = useHint(
    count === 1
      ? t("Another track plays into this one through its output or a send")
      : message("{count} other tracks play into this one through their outputs or sends", { count: plainNumber(count) })
  )
  return (
    <span
      data-slot="track-inputs"
      className={cn(
        "truncate px-1 text-[10px] leading-none text-muted-foreground",
        className
      )}
      {...hint}
    >
      {tracksLabel(count)}
    </span>
  )
}

/** What plays into a track. */
type Feeds = {
  channels: Channel[]
  clips: readonly ClipId[]
  /** Tracks that reach it through their output or a send. */
  inputs: number
}

/** Everything that plays into a track, one per line. */
export function FeedList({ channels, clips, inputs }: Feeds) {
  return (
    <ul className="flex flex-col gap-1">
      {channels.map((channel) => (
        <li key={channel.id} className="flex">
          <ChannelChip channel={channel} className="h-5 flex-1" />
        </li>
      ))}
      {clips.length > 0 && (
        <li className="flex">
          <AudioClipsChip clips={clips} className="h-5 flex-1" />
        </li>
      )}
      {inputs > 0 && (
        <li className="flex h-5 items-center">
          <FedByTracks count={inputs} />
        </li>
      )}
    </ul>
  )
}

/** What the count behind the first chip stands for, in a few words. */
function describe({ channels, clips, inputs }: Feeds): string {
  const parts: string[] = []
  if (channels.length > 0) {
    parts.push(
      channels.length === 1 ? "1 channel" : `${channels.length} channels`
    )
  }
  if (clips.length > 0) parts.push(clipsLabel(clips.length))
  if (inputs > 0) parts.push(inputs === 1 ? "1 track" : `${inputs} tracks`)
  return parts.join(", ")
}

function MoreFeeds({ feeds, more }: { feeds: Feeds; more: number }) {
  const { channels, clips, inputs } = feeds
  const onlyChannels = clips.length === 0 && inputs === 0
  const what = describe(feeds)
  const hint = useHint(message("{what} play into this track. Click to list them", { what }))
  return (
    <Popover>
      <PopoverTrigger
        render={
          <button
            type="button"
            aria-label={
              onlyChannels
                ? message("All {count} channels of this track", { count: plainNumber(channels.length) })
                : message("Everything that plays into this track: {what}", { what })
            }
            className="h-4 shrink-0 rounded-[3px] bg-muted px-1 font-readout text-[9px] text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring aria-expanded:text-foreground"
            {...hint}
          />
        }
      >
        +{more}
      </PopoverTrigger>
      <PopoverContent side="bottom" align="start" className="w-44 gap-1 p-1.5">
        <FeedList {...feeds} />
      </PopoverContent>
    </Popover>
  )
}

function Unused() {
  const hint = useHint(
    t("Nothing plays into this track. Route a channel or an audio clip to it, or another track's output or a send")
  )
  return (
    <span
      data-slot="track-unused"
      className="px-1 text-[10px] leading-none text-muted-foreground/70 italic"
      {...hint}
    >
      {t("unused")}
    </span>
  )
}

/**
 * The row under the name that shows what plays into the track: its channels
 * as chips, its audio clips and the tracks that feed it as counts, and
 * "unused" only when nothing at all does. The first of them is shown, and
 * a count beside it lists the rest. The master shows what is routed
 * straight to it; every track ends up there, so that is not counted.
 */
export function ChannelChips({ id, master }: { id: TrackId; master: boolean }) {
  const channels = useTrackChannels(id)
  const clips = useTrackAudioClips(id)
  const inputs = useProjectStore((state) =>
    master ? 0 : trackInputCount(state.project.mixer.tracks, id)
  )
  const feeds: Feeds = { channels, clips, inputs }
  // One line each: every channel, the audio clips, the tracks.
  const lines =
    channels.length + (clips.length > 0 ? 1 : 0) + (inputs > 0 ? 1 : 0)

  let first = null
  if (channels.length > 0) {
    first = <ChannelChip channel={channels[0]} className="flex-1" />
  } else if (clips.length > 0) {
    first = <AudioClipsChip clips={clips} className="flex-1" />
  } else if (inputs > 0) {
    first = <FedByTracks count={inputs} className="flex-1" />
  } else if (!master) {
    first = <Unused />
  }

  return (
    <div
      data-slot="track-channels"
      className="flex h-4 shrink-0 items-center gap-0.5 px-1"
    >
      {first}
      {lines > 1 && <MoreFeeds feeds={feeds} more={lines - 1} />}
    </div>
  )
}
