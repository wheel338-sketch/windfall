import type { ComponentType } from "react"

import BrowserPanel from "@/features/browser"
import ChannelRackPanel from "@/features/channel-rack"
import MixerPanel from "@/features/mixer"
import PianoRollPanel from "@/features/piano-roll"
import PlaylistPanel from "@/features/playlist"
import type { CenterTab, PanelId } from "@/lib/store/ui"
import { translatedText as t } from "@/lib/i18n"

export type { PanelId }
export { isPanelId } from "@/lib/store/ui"

type PanelInfo = {
  title: string
  /** Registry action that shows the panel, or toggles it for side panels. */
  action: string
  /** Reads everything from the stores, so it can be mounted in any window. */
  component: ComponentType
}

/** Every panel the workspace can show. */
export const PANELS: Record<PanelId, PanelInfo> = {
  browser: {
    get title() { return t("Browser") },
    action: "view.browser",
    component: BrowserPanel,
  },
  mixer: { get title() { return t("Mixer") }, action: "view.mixer", component: MixerPanel },
  channelRack: {
    get title() { return t("Channel rack") },
    action: "view.channelRack",
    component: ChannelRackPanel,
  },
  playlist: {
    get title() { return t("Playlist") },
    action: "view.playlist",
    component: PlaylistPanel,
  },
  pianoRoll: {
    get title() { return t("Piano roll") },
    action: "view.pianoRoll",
    component: PianoRollPanel,
  },
}

export const CENTER_TABS: CenterTab[] = ["channelRack", "playlist", "pianoRoll"]
