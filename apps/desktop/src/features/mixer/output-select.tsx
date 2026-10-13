import { ArrowRight02Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import type { MixerTrack, TrackId } from "@/bindings"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { useHint, useProjectStore } from "@/lib/store"
import { MASTER_TRACK } from "@/lib/units"
import { cn } from "@/lib/utils"

import { setOutput } from "./operations"
import { outputChoices } from "./routing"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const NONE = "none"

/** A track as a menu shows it: its place in the mixer, then its name. */
export function TrackLabel({
  track,
  number,
}: {
  track: MixerTrack
  number: number
}) {
  return (
    <span className="flex min-w-0 items-baseline gap-1.5">
      {track.id !== MASTER_TRACK && (
        <span className="w-5 shrink-0 text-right font-readout text-[10px] text-muted-foreground">
          {number}
        </span>
      )}
      <span className="truncate">{track.name}</span>
    </span>
  )
}

/** Why some tracks are missing from a routing menu. */
export function LoopNote({ count }: { count: number }) {
  if (count === 0) return null
  return (
    <p className="max-w-52 px-2 pt-1 pb-1.5 text-[10px] leading-snug text-muted-foreground">
      {count === 1
        ? t("1 track is not listed. It already plays into this one, so routing to it would loop.")
        : message("{count} tracks are not listed. They already play into this one, so routing to them would loop.", { count: plainNumber(count) })}
    </p>
  )
}

// Mounted only while the menu is open, so it can follow every track.
function OutputChoices({
  id,
  output,
}: {
  id: TrackId
  output: TrackId | null
}) {
  const tracks = useProjectStore((state) => state.project.mixer.tracks)
  const { tracks: open, looping } = outputChoices(tracks, id)
  return (
    <>
      <DropdownMenuRadioGroup
        value={output === null ? NONE : String(output)}
        onValueChange={(value: string) =>
          void setOutput(id, value === NONE ? null : Number(value))
        }
      >
        {open.map((track) => (
          <DropdownMenuRadioItem
            key={track.id}
            value={String(track.id)}
            closeOnClick
          >
            <TrackLabel track={track} number={tracks.indexOf(track)} />
          </DropdownMenuRadioItem>
        ))}
        <DropdownMenuSeparator />
        <DropdownMenuRadioItem value={NONE} closeOnClick>
          {t("None (sends only)")}
        </DropdownMenuRadioItem>
      </DropdownMenuRadioGroup>
      <LoopNote count={looping} />
    </>
  )
}

/** Picks where a track's output goes: the master, another track or nowhere. */
export function OutputSelect({
  id,
  output,
  className,
}: {
  id: TrackId
  output: TrackId | null
  className?: string
}) {
  const target = useProjectStore((state) =>
    output === null
      ? null
      : (state.project.mixer.tracks.find((track) => track.id === output)
          ?.name ?? null)
  )
  const hint = useHint(
    t("Where this track's sound goes next: the master, another track, or only its sends")
  )
  const label = target ?? t("No output")

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <button
            type="button"
            data-slot="track-output"
            aria-label={message("Output: {target}", { target: target ?? t("none") })}
            className={cn(
              "flex h-[18px] min-w-0 items-center gap-0.5 rounded-[3px] bg-muted/60 pr-1 pl-0.5 text-[10px] leading-none text-foreground/85 outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring aria-expanded:bg-muted aria-expanded:text-foreground",
              output === null && "text-muted-foreground italic",
              className
            )}
            {...hint}
          />
        }
      >
        <HugeiconsIcon
          icon={ArrowRight02Icon}
          strokeWidth={2}
          className="size-3 shrink-0 text-muted-foreground"
        />
        <span className="truncate">{label}</span>
      </DropdownMenuTrigger>
      <DropdownMenuContent className="max-h-80 w-auto min-w-44">
        <OutputChoices id={id} output={output} />
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
