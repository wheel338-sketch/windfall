import {
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import { useUiStore } from "@/lib/store/ui"
import { translatedText as t, dynamicTitle } from "@/lib/i18n"

/*
 * The right-click menus of the window's own chrome: the parts that belong
 * to no panel. Every entry is a registry action, so the menus show the same
 * titles, ticks and shortcuts as the menu bar and the palette.
 */

/** The tempo entries, shared by the transport bar and the tempo itself. */
export const TEMPO_MENU: ContextItem = {
  get submenu() { return t("Tempo") },
  items: ["tempo.tap", "tempo.half", "tempo.double", "tempo.reset"],
}

/** The title bar, around the menus and the search box. */
export const TITLE_MENU: ContextItem[] = [
  "view.commandPalette",
  contextSeparator,
  "view.browser",
  "view.mixer",
  contextSeparator,
  "view.toggleTheme",
  "view.resetLayout",
]

/** The transport bar, between its controls. */
export const TRANSPORT_MENU: ContextItem[] = [
  "transport.toggle",
  "transport.stop",
  contextSeparator,
  "transport.patternMode",
  "transport.songMode",
  "playlist.loopSong",
  "transport.metronome",
  contextSeparator,
  TEMPO_MENU,
  contextSeparator,
  "edit.undo",
  "edit.redo",
  "edit.history",
]

/** The strip of editor tabs. */
export const TABS_MENU: ContextItem[] = [
  "view.channelRack",
  "view.playlist",
  "view.pianoRoll",
  contextSeparator,
  "view.browser",
  "view.mixer",
  contextSeparator,
  "view.resetLayout",
]

/** The status bar. */
export const STATUS_MENU: ContextItem[] = [
  "options.settings",
  "engine.copyDeviceInfo",
  contextSeparator,
  "file.save",
  "file.saveAs",
]

/** The title strip of a docked panel, given the action that hides it. */
export function panelMenu(hideAction: string | undefined): ContextItem[] {
  return [
    ...(hideAction ? [hideAction, contextSeparator] : []),
    "view.browser",
    "view.mixer",
    contextSeparator,
    "view.resetLayout",
  ]
}

/**
 * The divider between two docked panels: put it back where it was, or hide
 * the panel it sizes. `group` is the panel group the divider belongs to.
 */
export function dividerMenu(group: string, hideAction: string): ContextItem[] {
  return [
    {
      ...dynamicTitle("Reset size", {}),
      run: () => useUiStore.getState().resetPanelSizes(group),
    },
    contextSeparator,
    hideAction,
    "view.resetLayout",
  ]
}

/** The start screen of a project with no channels. */
export const EMPTY_PROJECT_MENU: ContextItem[] = [
  "channel.add",
  "channel.addInstrument.subtractiveSynth",
  "channel.addFromFile",
  contextSeparator,
  "view.browser",
]
