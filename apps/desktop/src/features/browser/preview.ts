import type { LibraryFileToken } from "@/bindings"
import { reportError } from "@/lib/errors"
import { backend, errorMessage } from "@/lib/ipc"

import { createLatestRunner, type LatestRunner } from "./latest"
import { useBrowserStore } from "./store"
import { isUnder } from "./tree-model"
import {
  acceptFile,
  captureFile,
  fileRequestCurrent,
  resolveFile,
  type FileRequest,
} from "./file-token"
import { translatedText as t } from "@/lib/i18n"

/**
 * How long the selection must rest before a held arrow key plays or reads
 * anything. Key repeat fires about every 30 ms, so nothing is sent while the
 * key is down and the sound under the selection plays as soon as it stops.
 */
export const REPEAT_SETTLE_MS = 70

type PreviewJob =
  { type: "play"; path: string; file: FileRequest } | { type: "stop" }

type Timing = {
  browser?: LibraryFileToken
  /** True for a selection made by a repeating key. */
  settle?: boolean
}

const get = useBrowserStore.getState
const set = useBrowserStore.setState

let previewTimer: ReturnType<typeof setTimeout> | null = null
let infoTimer: ReturnType<typeof setTimeout> | null = null
let previewRunner: LatestRunner<PreviewJob>
let infoRunner: LatestRunner<{ path: string; file: FileRequest }>
let generation = 0

function createRunners() {
  generation += 1
  const mine = generation
  const current = () => mine === generation

  previewRunner = createLatestRunner<PreviewJob>(async (job, superseded) => {
    if (job.type === "stop") {
      try {
        await backend.previewStop()
      } catch (error) {
        if (current()) reportError(error, t("Could not stop the preview"))
      }
      return
    }
    try {
      const token = job.file.browser
        ? acceptFile(job.file, job.file.browser)
        : await resolveFile(job.file)
      if (!current() || superseded() || !fileRequestCurrent(job.file)) return
      await backend.previewPlay(job.path, token)
      // With a newer request waiting, this sound is about to be replaced.
      if (current() && !superseded() && fileRequestCurrent(job.file)) {
        set({
          playing: { path: job.path, startedAt: performance.now() },
          previewError: null,
        })
      }
    } catch (error) {
      if (current() && !superseded()) {
        set({
          playing: null,
          previewError: { path: job.path, message: errorMessage(error) },
        })
      }
    }
  })

  infoRunner = createLatestRunner<{ path: string; file: FileRequest }>(
    async ({ path, file }, superseded) => {
      try {
        const token = file.browser
          ? acceptFile(file, file.browser)
          : await resolveFile(file)
        if (!current() || superseded() || !fileRequestCurrent(file)) return
        const info = await backend.sampleInfo(path, token)
        if (!current() || superseded() || !fileRequestCurrent(file)) return
        // An answer for a sound the selection has left never reaches the pane.
        if (get().info?.path === path) {
          set({ info: { path, status: "ready", info } })
        }
      } catch (error) {
        if (current() && !superseded() && get().info?.path === path) {
          set({ info: { path, status: "error", message: errorMessage(error) } })
        }
      }
    }
  )
}

createRunners()

function clearPreviewTimer() {
  if (previewTimer !== null) clearTimeout(previewTimer)
  previewTimer = null
}

function clearInfoTimer() {
  if (infoTimer !== null) clearTimeout(infoTimer)
  infoTimer = null
}

/** Plays a sound through the engine's preview voice, replacing the last one. */
export function requestPreview(path: string, timing: Timing = {}) {
  const file = captureFile(path, timing.browser)
  clearPreviewTimer()
  if (!timing.settle) {
    previewRunner.request({ type: "play", path, file })
    return
  }
  previewRunner.cancel()
  previewTimer = setTimeout(() => {
    previewTimer = null
    previewRunner.request({ type: "play", path, file })
  }, REPEAT_SETTLE_MS)
}

export function stopPreview() {
  clearPreviewTimer()
  set({ playing: null })
  previewRunner.request({ type: "stop" })
}

/** The pane calls this when the sound has played to its end. */
export function previewEnded(path: string) {
  if (get().playing?.path === path) set({ playing: null })
}

/** Makes the pane show a sound's facts and waveform, reading them if needed. */
export function requestInfo(path: string, timing: Timing = {}) {
  const file = captureFile(path, timing.browser)
  clearInfoTimer()
  infoRunner.cancel()
  set({ info: { path, status: "loading" } })
  if (!timing.settle) {
    infoRunner.request({ path, file })
    return
  }
  infoTimer = setTimeout(() => {
    infoTimer = null
    infoRunner.request({ path, file })
  }, REPEAT_SETTLE_MS)
}

export function clearInfo() {
  clearInfoTimer()
  infoRunner.cancel()
  if (get().info !== null) set({ info: null })
}

/** Clears displayed facts for files in a folder that is being read again. */
export function forgetInfoUnder(folder: string) {
  if (get().info && isUnder(get().info!.path, folder)) clearInfo()
}

/** Starts over with no requests under way. For tests. */
export function resetPreview() {
  clearPreviewTimer()
  clearInfoTimer()
  createRunners()
}
