import {
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"

import { toolActionId } from "./actions"
import { TOOLS } from "./intents"
import { translatedText as t } from "@/lib/i18n"

/**
 * Everywhere in the playlist that has no menu of its own: the toolbar, the
 * pattern list around its patterns, the scrollbars and the corners. The
 * clips, the ruler and the track names each have theirs.
 */
export const PANEL_MENU: ContextItem[] = [
  { get submenu() { return t("Tool") }, items: TOOLS.map(toolActionId) },
  contextSeparator,
  "playlist.paste",
  "playlist.selectAll",
  "playlist.selectMatchingClips",
  "playlist.selectAtPlayhead",
  "playlist.selectMutedClips",
  "playlist.selectClipsOnMutedTracks",
  "playlist.makeUnique",
  "playlist.bounceSelectedClips",
  "playlist.consolidateSelectedTracks",
  "playlist.addTrack",
  "playlist.addInstrumentTrack",
  "playlist.addAudioTrack",
  contextSeparator,
  "playlist.zoomToFit",
  "playlist.follow",
  "playlist.loopSong",
  "playlist.patterns",
]
