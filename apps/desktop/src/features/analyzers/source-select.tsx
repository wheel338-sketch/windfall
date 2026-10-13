import type { MixerTrack } from "@/bindings"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { useProjectStore } from "@/lib/store/project"
import { MASTER_TRACK } from "@/lib/units"
import { cn } from "@/lib/utils"

import type { AnalyzerSource } from "./types"
import { translatedText as t, message } from "@/lib/i18n"

const MIX = "mix"

/** A source as a menu's value: "mix", or the track's id. */
export function sourceValue(source: AnalyzerSource): string {
  return source.type === "mixOutput" ? MIX : String(source.track)
}

/** The source a menu value stands for. */
export function sourceOf(value: string): AnalyzerSource {
  return value === MIX
    ? { type: "mixOutput" }
    : { type: "trackPostFader", track: Number(value) }
}

/**
 * What a source is called. The mix output and the master track are not the
 * same thing: the master track is the mix before the output gain, and
 * without what plays past the mixer.
 */
export function sourceLabel(
  source: AnalyzerSource,
  tracks: readonly MixerTrack[]
): string {
  if (source.type === "mixOutput") return t("Mix output")
  const track = tracks.find((item) => item.id === source.track)
  if (!track) return t("A track that is gone")
  return track.id === MASTER_TRACK
    ? t("Master track, after its fader")
    : message("{trackName}, after its fader", { trackName: track.name })
}

function Choices({ tracks }: { tracks: readonly MixerTrack[] }) {
  return (
    <>
      <DropdownMenuRadioItem value={MIX} closeOnClick>
        {t("Mix output")}
      </DropdownMenuRadioItem>
      <DropdownMenuSeparator />
      {tracks.map((track) => (
        <DropdownMenuRadioItem
          key={track.id}
          value={String(track.id)}
          closeOnClick
        >
          <span className="truncate">
            {track.id === MASTER_TRACK ? t("Master track") : track.name}
          </span>
        </DropdownMenuRadioItem>
      ))}
    </>
  )
}

type SourceSelectProps = {
  source: AnalyzerSource
  onChange(source: AnalyzerSource): void
  className?: string
}

/**
 * Picks what an analyzer view listens to: the mix output, or one mixer
 * track after its effects, fader and pan. It is a menu button, so it opens
 * with Enter or Space and is walked with the arrow keys.
 */
export function AnalyzerSourceSelect({
  source,
  onChange,
  className,
}: SourceSelectProps) {
  const tracks = useProjectStore((state) => state.project.mixer.tracks)
  const label = sourceLabel(source, tracks)
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <button
            type="button"
            data-slot="analyzer-source"
            data-source={sourceValue(source)}
            aria-label={message("Source: {label}", { label })}
            className={cn(
              "flex h-7 min-w-0 items-center gap-1.5 rounded-md border border-input bg-input/20 px-2 text-xs outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring aria-expanded:bg-muted",
              className
            )}
          />
        }
      >
        <span className="shrink-0 text-muted-foreground">{t("Source")}</span>
        <span className="truncate">{label}</span>
      </DropdownMenuTrigger>
      <DropdownMenuContent className="max-h-80 w-auto min-w-52">
        <DropdownMenuRadioGroup
          value={sourceValue(source)}
          onValueChange={(value: string) => onChange(sourceOf(value))}
        >
          <Choices tracks={tracks} />
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
