import { toast } from "sonner"
import { openPresetRender } from "@/features/preset-render/dialog"
import { isPresetPath } from "@/features/preset-render/types"

import type {
  BrowserRoot,
  Channel,
  DispatchResult,
  LibraryFileToken,
} from "@/bindings"
import { addAudioFile } from "@/features/playlist/audio/ops"
import { openStemClip, useAnalysisDialog } from "@/features/analysis/actions"
import { useRecordingStore } from "@/features/transport/recording-store"
import { usePlaylistStore } from "@/features/playlist/store"
import { songTick } from "@/features/playlist/ops"
import { revealClip } from "@/features/playlist/reveal"
import { attempt, reportError } from "@/lib/errors"
import { openProjectPath } from "@/lib/flows/project"
import { backend } from "@/lib/ipc"
import { receivePatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { useUiStore } from "@/lib/store/ui"

import {
  clearInfo,
  forgetInfoUnder,
  requestInfo,
  requestPreview,
  stopPreview,
} from "./preview"
import {
  applyRoots,
  expandFolder,
  loadRoots,
  refreshFolder,
  revealRow,
  selectionOf,
  setFilter,
  toggleFolder,
  useBrowserStore,
  type Selection,
} from "./store"
import { rowId, type EntryRow } from "./tree-model"
import {
  invalidateLibrary,
  refreshLibrary,
  useLibraryStore,
} from "./library-store"
import { captureFile, fileRequestCurrent, resolveFile } from "./file-token"
import { translatedText as t, message } from "@/lib/i18n"

const get = useBrowserStore.getState
const set = useBrowserStore.setState

type SelectOptions = {
  /** Play the sound, when auto-preview is on. */
  preview?: boolean
  /** The selection is moving under a held key; wait for it to rest. */
  settle?: boolean
}

/**
 * Moves the selection to a row. This is the heart of auditioning: landing on
 * a sound shows it in the pane and, with auto-preview on, plays it.
 */
export function selectRow(row: EntryRow, options: SelectOptions = {}) {
  const selected = selectionOf(row)
  set({ selected })
  if (row.kind !== "audio" || isPresetPath(row.path)) {
    clearInfo()
    return
  }
  requestInfo(row.path, { settle: options.settle, browser: selected.library })
  if (options.preview && get().autoPreview) {
    requestPreview(row.path, {
      settle: options.settle,
      browser: selected.library,
    })
  }
}

/** The selected row when it is a sound, otherwise null. */
export function selectedSound(): Selection | null {
  const selected = get().selected
  return selected?.kind === "audio" ? selected : null
}

export function selectedChannel(): Channel | null {
  const id = useUiStore.getState().selectedChannel
  if (id === null) return null
  return (
    useProjectStore
      .getState()
      .project.channels.find((channel) => channel.id === id) ?? null
  )
}

function applyResult(result: DispatchResult) {
  // The same patch also arrives as an event; the store ignores the repeat.
  receivePatch(result.patch)
}

/** Adds a new channel that plays the file, and selects it in the rack. */
export async function addToRack(
  path: string,
  browser?: LibraryFileToken
): Promise<void> {
  const generation = getProjectGeneration()
  const file = captureFile(path, browser)
  const token = await attempt(
    resolveFile(file),
    t("Could not find the library file")
  )
  if (
    !token ||
    generation !== getProjectGeneration() ||
    !fileRequestCurrent(file)
  )
    return
  const result = await attempt(
    isPresetPath(path)
      ? openPresetRender({ path, browser: token }, undefined,
          () => generation === getProjectGeneration() && fileRequestCurrent(file))
      : backend.addChannelFromFile(path, undefined, token),
    t("Could not add the sound")
  )
  if (
    !result ||
    generation !== getProjectGeneration() ||
    !fileRequestCurrent(file)
  )
    return
  applyResult(result)
  const channels = useProjectStore.getState().project.channels
  const created = result.created.find((id) =>
    channels.some((channel) => channel.id === id)
  )
  const ui = useUiStore.getState()
  if (created !== undefined) ui.selectChannel(created)
  ui.showCenterTab("channelRack")
}

/**
 * Puts the file on the playlist as an audio clip: at the song position, on
 * a new track of its own, playing into a new mixer track. Shows the
 * playlist with the clip selected.
 */
export async function addToPlaylist(
  path: string,
  browser?: LibraryFileToken
): Promise<void> {
  const generation = getProjectGeneration()
  const file = captureFile(path, browser)
  const token = await attempt(
    resolveFile(file),
    t("Could not find the library file")
  )
  if (
    !token ||
    generation !== getProjectGeneration() ||
    !fileRequestCurrent(file)
  )
    return
  const clip = await addAudioFile(
    path,
    { start: songTick() },
    token,
    () => generation === getProjectGeneration() && fileRequestCurrent(file)
  )
  if (
    clip !== null &&
    generation === getProjectGeneration() &&
    fileRequestCurrent(file)
  )
    revealClip(clip)
}

/** Explicit browser import followed by the same checked playlist stem review. */
export async function separateToPlaylist(path: string, browser?: LibraryFileToken): Promise<void> {
  const idle = () => !useRecordingStore.getState().state.active && !useRecordingStore.getState().busy && useAnalysisDialog.getState().target === null
  if (!idle()) return
  const generation = getProjectGeneration()
  const file = captureFile(path, browser)
  const token = await attempt(resolveFile(file), t("Could not find the library file"))
  const valid = () => idle() && generation === getProjectGeneration() && fileRequestCurrent(file)
  if (!token || !valid()) return
  const clip = await addAudioFile(path, { start: songTick() }, token, valid)
  if (clip === null || !valid()) return
  revealClip(clip)
  usePlaylistStore.getState().select([clip])
  openStemClip(clip)
}

/** Makes the channel selected in the rack play the file instead. */
export async function replaceChannelSample(
  path: string,
  browser?: LibraryFileToken
): Promise<void> {
  const generation = getProjectGeneration()
  const file = captureFile(path, browser)
  const channel = selectedChannel()
  if (!channel) return
  const token = await attempt(
    resolveFile(file),
    t("Could not find the library file")
  )
  if (
    !token ||
    generation !== getProjectGeneration() ||
    !fileRequestCurrent(file)
  )
    return
  const result = await attempt(
    isPresetPath(path)
      ? openPresetRender({ path, channel: channel.id, browser: token }, undefined,
          () => generation === getProjectGeneration() && fileRequestCurrent(file))
      : backend.setChannelSampleFromFile(channel.id, path, token),
    message("Could not replace the sample of {channelName}", { channelName: channel.name })
  )
  if (
    result &&
    generation === getProjectGeneration() &&
    fileRequestCurrent(file)
  )
    applyResult(result)
}

/** What Enter and a double-click do. Folders open on a single click instead. */
export function activateRow(row: EntryRow) {
  if (row.kind === "audio") void addToRack(row.path, row.library)
  else if (row.kind === "project") void openProjectPath(row.path)
  else if (row.kind === "folder") toggleFolder(row)
}

export function refresh(path: string) {
  forgetInfoUnder(path)
  refreshFolder(path)
}

/**
 * Refreshes the selected folder, or the folder the selected file is in. With
 * nothing selected it reads the roots and every open root again.
 */
export function refreshSelection() {
  void attempt(refreshLibrary(), t("Could not refresh the library"))
  const selected = get().selected
  if (selected === null) {
    void loadRoots().then(() => {
      const { roots, expanded } = get()
      for (const root of roots) {
        if (expanded.has(rowId(root.path, root.path))) refresh(root.path)
      }
    })
    return
  }
  const path = selected.kind === "folder" ? selected.path : selected.parentPath
  if (path !== null) refresh(path)
}

/** Asks for a folder and adds it to the browser, open and selected. */
export async function addFolder(): Promise<void> {
  let path: string | null
  try {
    path = await backend.pickFolder()
  } catch (error) {
    reportError(error, t("Could not choose a folder"))
    return
  }
  if (path === null) return

  let roots: BrowserRoot[]
  try {
    invalidateLibrary()
    roots = await backend.browserAddRoot(path)
  } catch (error) {
    reportError(error, t("Could not add the folder"))
    return
  }

  const known = new Set(get().roots.map((root) => root.path))
  applyRoots(roots)
  const added = roots.find((root) => !known.has(root.path))
  if (!added) return

  const id = rowId(added.path, added.path)
  // A filter would hide the folder that was just added.
  useLibraryStore.setState({ favoritesOnly: false, tags: [] })
  setFilter("")
  expandFolder(id, added.path)
  // It may have been in the browser before, with a listing that is now old.
  refresh(added.path)
  set({
    selected: {
      id,
      path: added.path,
      name: added.name,
      kind: "folder",
      parentPath: null,
      root: added,
    },
  })
  clearInfo()
  revealRow(id)
}

/** Takes a folder the user added out of the browser. Its files stay on disk. */
export async function removeRoot(root: BrowserRoot): Promise<void> {
  if (root.kind === "factory") return
  try {
    invalidateLibrary()
    applyRoots(await backend.browserRemoveRoot(root.path))
    if (get().selected === null) {
      clearInfo()
      stopPreview()
    }
  } catch (error) {
    reportError(error, message("Could not remove {rootName}", { rootName: root.name }))
  }
}

export async function copyPath(path: string): Promise<void> {
  await navigator.clipboard.writeText(path)
  toast.success(t("Path copied"), { description: path })
}
