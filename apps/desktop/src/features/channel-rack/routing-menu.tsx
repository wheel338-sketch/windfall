import type { ChannelId, TrackId } from "@/bindings"
import { ActionMenuItem } from "@/components/action-menu-item"
import {
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
} from "@/components/ui/dropdown-menu"
import { useActions, useAppState } from "@/lib/actions"
import {
  useMixerTrack,
  useMixerTrackIds,
  useMixerTrackNumber,
} from "@/lib/store/selectors"
import { colorToCss } from "@/lib/units"

import { routeChannel } from "./channel-ops"
import { translatedText as t } from "@/lib/i18n"

/** What a mixer track is called in a small space: "M" for the master, else its number. */
export function trackNumber(index: number): string {
  if (index < 0) return "?"
  return index === 0 ? "M" : String(index)
}

function TrackOption({ id }: { id: TrackId }) {
  const track = useMixerTrack(id)
  const index = useMixerTrackNumber(id)
  if (!track || track.current) return null
  return (
    <DropdownMenuRadioItem value={id} closeOnClick>
      <span
        aria-hidden
        className="size-2 shrink-0 rounded-[2px]"
        style={{ backgroundColor: colorToCss(track.color) }}
      />
      <span className="w-5 shrink-0 font-readout text-muted-foreground">
        {trackNumber(index)}
      </span>
      <span className="truncate">{track.name}</span>
    </DropdownMenuRadioItem>
  )
}

const ROUTING_ACTIONS = ["channel.routeToNewTrack", "channel.showInMixer"]

type RoutingMenuProps = {
  channel: ChannelId
  channelName: string
  track: TrackId
}

/**
 * The inside of a routing menu: every mixer track to choose from, then the
 * routing actions. It is rendered only while its menu is open, and the menu
 * selects the channel first, so the actions act on it.
 */
export function RoutingMenu({ channel, channelName, track }: RoutingMenuProps) {
  const ids = useMixerTrackIds()
  const actions = useActions()
  const state = useAppState()

  return (
    <>
      <DropdownMenuGroup>
        <DropdownMenuLabel>{channelName}{" "}{t("plays into")}</DropdownMenuLabel>
        <DropdownMenuRadioGroup
          value={track}
          onValueChange={(id: TrackId) => void routeChannel(channel, id)}
        >
          {ids.map((id) => (
            <TrackOption key={id} id={id} />
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuGroup>
      <DropdownMenuSeparator />
      {ROUTING_ACTIONS.map((id) => {
        const action = actions.find((item) => item.id === id)
        return (
          action && <ActionMenuItem key={id} action={action} state={state} />
        )
      })}
    </>
  )
}
