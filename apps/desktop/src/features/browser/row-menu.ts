import type { Channel } from "@/bindings"
import {
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import { openProjectPath } from "@/lib/flows/project"

import {
  addToPlaylist,
  separateToPlaylist,
  addToRack,
  copyPath,
  refresh,
  removeRoot,
  replaceChannelSample,
} from "./commands"
import { addChannelFromSoundFont } from "@/features/sample-instruments/soundfont-channel"

import { requestPreview } from "./preview"
import { toggleFolder } from "./store"
import type { EntryRow } from "./tree-model"
import { dynamicTitle } from "@/lib/i18n"

/** The browser's empty space, and its parts that have no menu of their own. */
export const BROWSER_MENU: ContextItem[] = [
  "browser.addFolder",
  "browser.refresh",
  "browser.collapseFolders",
  "browser.expandLoadedFolders",
  contextSeparator,
  "browser.toggleAutoPreview",
  "browser.focusSearch",
  contextSeparator,
  "view.browser",
]

/**
 * What a right-click offers for a row, or for the empty space below the
 * rows when `row` is null.
 */
export function rowMenu(
  row: EntryRow | null,
  channel: Channel | null
): ContextItem[] {
  if (row === null) return BROWSER_MENU

  const copy: ContextItem = {
    ...dynamicTitle("Copy path", {}),
    run: () => copyPath(row.path),
  }

  if (row.kind === "audio") {
    return [
      {
        ...dynamicTitle("Preview", {}),
        run: () => requestPreview(row.path, { browser: row.library }),
      },
      {
        ...dynamicTitle("Add to new channel", {}),
        shortcut: "Enter",
        run: () => addToRack(row.path, row.library),
      },
      {
        ...dynamicTitle("Add to playlist", {}),
        run: () => addToPlaylist(row.path, row.library),
      },
      { ...dynamicTitle("Separate into four stems", {}), run: () => separateToPlaylist(row.path, row.library) },
      {
        ...(channel
          ? dynamicTitle("Replace sample of {channelName}", { channelName: channel.name })
          : dynamicTitle("Replace selected channel's sample", {})),
        // An instrument has no sample to replace.
        disabled: channel?.source.type !== "sampler",
        shortcut:
          channel?.source.type === "instrument" ? "samplers only" : undefined,
        run: () => replaceChannelSample(row.path, row.library),
      },
      contextSeparator,
      copy,
    ]
  }

  if (row.kind === "project") {
    return [
      {
        ...dynamicTitle("Open project", {}),
        shortcut: "Enter",
        run: () => openProjectPath(row.path),
      },
      contextSeparator,
      copy,
    ]
  }

  if (row.kind === "other") return [copy]

  if (row.kind === "soundFont") {
    return [
      {
        ...dynamicTitle("Add a channel from this SoundFont…", {}),
        shortcut: "Enter",
        run: () => addChannelFromSoundFont(row.path),
      },
      contextSeparator,
      copy,
    ]
  }

  const items: ContextItem[] = [
    {
      ...dynamicTitle(row.open ? "Collapse" : "Expand", {}),
      // The filter is holding it open to show what it found inside.
      disabled: row.forced,
      run: () => toggleFolder(row),
    },
    { ...dynamicTitle("Refresh", {}), run: () => refresh(row.path) },
    contextSeparator,
    copy,
  ]
  const root = row.root
  if (root !== null) {
    items.push(contextSeparator, "browser.addFolder")
    if (root.kind === "user") {
      items.push({
        ...dynamicTitle("Remove from browser", {}),
        destructive: true,
        run: () => removeRoot(root),
      })
    }
  }
  return items
}
