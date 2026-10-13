import { invalidateActionsOn, registry, type Action } from "@/lib/actions"
import { useUiStore } from "@/lib/store/ui"

import {
  addFolder,
  addToPlaylist,
  addToRack,
  refreshSelection,
  replaceChannelSample,
  selectedChannel,
  selectedSound,
} from "./commands"
import { requestPreview, stopPreview } from "./preview"
import {
  collapseAllFolders,
  expandLoadedFolders,
  requestFilterFocus,
  setAutoPreview,
  useBrowserStore,
} from "./store"
import { dynamicTitle, translatedText as t } from "@/lib/i18n"

const SECTION = "Browser"

/**
 * What the browser adds to the command palette, the menus and the keymap.
 * Actions on one row (expand, copy path, remove a folder) are in that row's
 * right-click menu instead.
 */
export const BROWSER_ACTIONS: Action[] = [
  {
    id: "browser.addFolder",
    ...dynamicTitle("Add folder to the browser…", {}),
    section: SECTION,
    keywords: "samples library location directory user",
    run: addFolder,
  },
  {
    id: "browser.focusSearch",
    ...dynamicTitle("Filter the browser", {}),
    section: SECTION,
    defaultShortcut: "Mod+F",
    keywords: "search find samples sounds",
    run: () => {
      // The filter box takes the focus once the panel is on screen.
      useUiStore.getState().setPanelVisible("browser", true)
      requestFilterFocus()
    },
  },
  {
    id: "browser.toggleAutoPreview",
    ...dynamicTitle("Preview sounds when selected", {}),
    section: SECTION,
    keywords: "audition auto play listen",
    checked: () => useBrowserStore.getState().autoPreview,
    run: () => {
      const next = !useBrowserStore.getState().autoPreview
      setAutoPreview(next)
      if (!next) stopPreview()
    },
  },
  {
    id: "browser.stopPreview",
    ...dynamicTitle("Stop preview", {}),
    section: SECTION,
    defaultShortcut: "Escape",
    keywords: "audition silence",
    run: stopPreview,
  },
  {
    id: "browser.previewSelected",
    ...dynamicTitle("Preview selected sound", {}),
    section: SECTION,
    keywords: "audition play listen",
    enabled: () => selectedSound() !== null,
    run: () => {
      const sound = selectedSound()
      if (sound) requestPreview(sound.path)
    },
  },
  {
    id: "browser.addSelectedToRack",
    ...dynamicTitle("Add selected sound to the rack", {}),
    section: SECTION,
    keywords: "new channel sample load",
    enabled: () => selectedSound() !== null,
    run: async () => {
      const sound = selectedSound()
      if (sound) await addToRack(sound.path)
    },
  },
  {
    id: "browser.addSelectedToPlaylist",
    ...dynamicTitle("Add selected sound to the playlist", {}),
    section: SECTION,
    keywords: "audio clip timeline song place",
    enabled: () => selectedSound() !== null,
    run: async () => {
      const sound = selectedSound()
      if (sound) await addToPlaylist(sound.path)
    },
  },
  {
    id: "browser.replaceChannelSample",
    ...dynamicTitle("Replace selected channel's sample", {}),
    section: SECTION,
    keywords: "swap sound load",
    enabled: () =>
      selectedSound() !== null && selectedChannel()?.source.type === "sampler",
    whyDisabled: () =>
      selectedChannel()?.source.type === "instrument"
        ? t("Samplers only")
        : undefined,
    run: async () => {
      const sound = selectedSound()
      if (sound) await replaceChannelSample(sound.path)
    },
  },
  {
    id: "browser.refresh",
    ...dynamicTitle("Refresh browser folder", {}),
    section: SECTION,
    keywords: "reload rescan read again",
    run: refreshSelection,
  },
  {
    id: "browser.collapseFolders",
    ...dynamicTitle("Collapse folders", {}),
    section: SECTION,
    keywords: "close tree directories",
    run: collapseAllFolders,
  },
  {
    id: "browser.expandLoadedFolders",
    ...dynamicTitle("Expand loaded folders", {}),
    section: SECTION,
    keywords: "open tree directories",
    run: expandLoadedFolders,
  },
]

/** Adds the browser's actions. Returns a function that removes them again. */
export function registerBrowserActions(): () => void {
  const stops = [
    registry.register(BROWSER_ACTIONS),
    invalidateActionsOn(useBrowserStore, (state) => [
      state.autoPreview,
      state.selected,
    ]),
  ]
  return () => {
    for (const stop of stops) stop()
  }
}
