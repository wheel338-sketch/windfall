import { watchMixerWaveforms } from "@/features/mixer/waveform-meter"
import { reportError } from "@/lib/errors"
import { watchCurrentSource } from "@/features/mixer/current-source"
import { logHardwareNote } from "@/features/piano-roll/note-log"
import { backend } from "@/lib/ipc"

import { receiveEngineStatus, refreshEngineStatus } from "./engine"
import { loadSnapshot, receivePatch, refetchSnapshot } from "./project"
import { announceProjectReplaced } from "./replaced"
import { receiveTransportState, refreshTransport } from "./transport"
import { useUiStore } from "./ui"
import { clearWarnings, receiveWarnings, samplesReloaded } from "./warnings"

const LATENCY_REFRESH_MS = 150

/**
 * Wires the stores to the backend: loads the current state and keeps it up
 * to date from the backend's events. Call once per window. Returns a
 * function that disconnects.
 */
export function connectStores(): () => void {
  // The delay effects and instruments add is part of the engine's status
  // and changes with the project, so the status is read again after an edit
  // that could move it: once per burst, since a drag sends many.
  let latencyTimer: ReturnType<typeof setTimeout> | null = null
  const refreshLatencySoon = () => {
    if (latencyTimer !== null) return
    latencyTimer = setTimeout(() => {
      latencyTimer = null
      void refreshEngineStatus()
    }, LATENCY_REFRESH_MS)
  }

  const disconnect = [
    watchCurrentSource(),
    watchMixerWaveforms(),
    backend.onPanelDocked((id) =>
      useUiStore.getState().removeDetachedPanel(id)
    ),
    backend.onProjectPatch((patch) => {
      receivePatch(patch)
      if (patch.mixer || patch.channels) refreshLatencySoon()
    }),
    backend.onProjectLoaded((snapshot) => {
      loadSnapshot(snapshot)
      // Ids from the previous project mean nothing in the new one.
      useUiStore.setState({ selectedChannel: null, selectedTrack: null })
      clearWarnings()
      samplesReloaded()
      refreshLatencySoon()
      // Last, so every listener already sees the new project in the stores.
      announceProjectReplaced()
    }),
    backend.onTransportState(receiveTransportState),
    backend.onEngineStatus(receiveEngineStatus),
    backend.onMidiHardwareNote((note) =>
      logHardwareNote(note.channel, note.key, note.velocity)
    ),
    backend.onProjectWarnings((warnings) => {
      receiveWarnings(warnings)
      for (const warning of warnings) reportError(warning, "Project")
    }),
  ]

  void refetchSnapshot()
  void refreshTransport()
  void refreshEngineStatus()

  return () => {
    if (latencyTimer !== null) clearTimeout(latencyTimer)
    for (const off of disconnect) off()
  }
}
