import { create } from "zustand"
import { toast } from "sonner"
import type { RecordingInput } from "@/bindings"
import { backend, errorSource } from "@/lib/ipc"
import { receivePatch } from "@/lib/store/project"
import { usePlaylistStore } from "@/features/playlist/store"
import type {
  AudioLoggerSettings,
  AudioLoggerSource,
  AudioLoggerState,
} from "./types"
import { translatedText as t } from "@/lib/i18n"

/**
 * The audio logger as the dialog shows it. `state` is what the native
 * logger last reported; nothing here is project data.
 */
export const useAudioLoggerStore = create(() => ({
  open: false,
  busy: false,
  state: null as AudioLoggerState | null,
  inputs: [] as RecordingInput[],
  /** Trailing seconds a dump keeps; 0 keeps everything. */
  dumpSeconds: 30,
  error: "",
}))

async function work(action: () => Promise<void>) {
  useAudioLoggerStore.setState({ busy: true, error: "" })
  try {
    await action()
  } catch (error) {
    useAudioLoggerStore.setState({ error: errorSource(error) })
  } finally {
    useAudioLoggerStore.setState({ busy: false })
  }
}

/** Reads the logger's state. A failure is shown, not thrown. */
export async function refreshAudioLogger() {
  try {
    useAudioLoggerStore.setState({ state: await backend.audioLoggerState() })
  } catch (error) {
    useAudioLoggerStore.setState({ error: errorSource(error) })
  }
}

export async function openAudioLogger() {
  useAudioLoggerStore.setState({ open: true, error: "" })
  await work(async () => {
    const [state, inputs] = await Promise.all([
      backend.audioLoggerState(),
      backend.recordingInputs(),
    ])
    useAudioLoggerStore.setState({ state, inputs })
  })
}

export function closeAudioLogger() {
  useAudioLoggerStore.setState({ open: false })
}

/** Changes some of the settings and keeps the rest as the logger has them. */
export function configureAudioLogger(patch: Partial<AudioLoggerSettings>) {
  return work(async () => {
    const current = useAudioLoggerStore.getState().state
    if (!current) return
    const state = await backend.audioLoggerConfigure({
      ...current.settings,
      ...patch,
    })
    useAudioLoggerStore.setState({ state })
  })
}

/** Puts the end of one history on the playlist at the playhead. */
export function dumpAudioLogger(source: AudioLoggerSource) {
  return work(async () => {
    const seconds = useAudioLoggerStore.getState().dumpSeconds
    const result = await backend.audioLoggerDump({
      source,
      seconds: seconds > 0 ? seconds : null,
      start: null,
      track: null,
    })
    receivePatch(result.patch)
    // The clip is created last. Selecting it puts it in the clip inspector,
    // where the audio editor opens it.
    const clip = result.created.at(-1)
    if (clip !== undefined) usePlaylistStore.getState().select([clip])
    toast.success(t("Logged audio added to playlist"))
    await refreshAudioLogger()
  })
}
