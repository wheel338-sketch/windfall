import { create } from "zustand"

import type { AudioHost, AudioSettings, EngineStatus } from "@/bindings"
import { attempt, reportError } from "@/lib/errors"
import { backend, type StoredAudioSettings } from "@/lib/ipc"

type EngineState = {
  /** Null until the backend has reported once. */
  status: EngineStatus | null
  /**
   * The output the user asked for. A field that is null or missing means
   * "the default". Null until it has been read.
   */
  request: StoredAudioSettings | null
  /** Audio drivers and their devices. Loaded when the settings open. */
  hosts: AudioHost[]
}

export const useEngineStore = create<EngineState>(() => ({
  status: null,
  request: null,
  hosts: [],
}))

/** Counts `engine:status` events, to tell which news is newer. */
let eventsSeen = 0

/** Takes the status from an `engine:status` event. Events are the truth. */
export function receiveEngineStatus(status: EngineStatus) {
  eventsSeen += 1
  useEngineStore.setState({ status })
}

/**
 * Uses the status a command replied with, unless an event has come in since
 * the command was sent: the event is then the newer of the two.
 */
async function replyStatus(
  work: () => Promise<EngineStatus>
): Promise<EngineStatus> {
  const sentAt = eventsSeen
  const status = await work()
  if (eventsSeen === sentAt) useEngineStore.setState({ status })
  return status
}

/** Reads the engine's status from the backend. Used once, at startup. */
export async function refreshEngineStatus(): Promise<void> {
  try {
    await replyStatus(() => backend.engineStatus())
  } catch (error) {
    reportError(error, "Could not read the audio engine")
  }
}

/** Loads the devices and the stored request, for the settings dialog. */
export async function loadAudioDevices(): Promise<void> {
  const [hosts, request] = await Promise.all([
    attempt(backend.engineDevices(), "Could not list audio devices"),
    attempt(backend.engineSettings(), "Could not read the audio settings"),
  ])
  if (hosts) useEngineStore.setState({ hosts })
  if (request) useEngineStore.setState({ request })
}

/** Opens the audio output the user asked for and reports how it went. */
export async function configureEngine(
  settings: AudioSettings
): Promise<EngineStatus | null> {
  try {
    const status = await replyStatus(() => backend.engineConfigure(settings))
    useEngineStore.setState({
      request: status.error ? await backend.engineSettings() : settings,
    })
    return status
  } catch (error) {
    reportError(error, "Could not change the audio device")
    return null
  }
}
