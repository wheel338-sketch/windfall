import { create } from "zustand"

import type {
  PatternId,
  MetronomeSettings,
  PlayMode,
  TransportPatch,
  TransportState,
} from "@/bindings"
import { reportError } from "@/lib/errors"
import { backend } from "@/lib/ipc"

export const DEFAULT_METRONOME: MetronomeSettings = { enabled: false, gain: 0.25, accent: true }
export function setMetronome(patch: Partial<MetronomeSettings>): Promise<void> {
  return setTransport({ metronome: { ...DEFAULT_METRONOME, ...useTransportStore.getState().metronome, ...patch } })
}

/** What the transport is doing, as last reported by the backend. */
export const useTransportStore = create<TransportState>(() => ({
  playing: false,
  mode: "pattern",
  pattern: 0,
  loopSong: true,
}))

/** Counts `transport:state` events, to tell which news is newer. */
let eventsSeen = 0

/** Takes the state from a `transport:state` event. Events are the truth. */
export function receiveTransportState(state: TransportState) {
  eventsSeen += 1
  useTransportStore.setState(state)
}

/**
 * Sends a transport command. The reply holds the state as it was when the
 * command ran, and it can arrive after an event that is newer: the engine
 * stops at once when there is nothing to play, and that "stopped" event can
 * overtake the "playing" reply. So the reply is used only when no event has
 * come in since the command was sent.
 */
async function run(work: () => Promise<TransportState>, what?: string) {
  const sentAt = eventsSeen
  try {
    const state = await work()
    if (eventsSeen === sentAt) useTransportStore.setState(state)
  } catch (error) {
    reportError(error, what)
  }
}

/** Reads the transport from the backend. Used once, at startup. */
export function refreshTransport(): Promise<void> {
  return run(() => backend.transportState(), "Could not read the transport")
}

export function play(): Promise<void> {
  return run(() => backend.transportPlay())
}

export function stop(): Promise<void> {
  return run(() => backend.transportStop())
}

export function togglePlayback(): Promise<void> {
  return run(() => backend.transportToggle())
}

export function setTransport(patch: TransportPatch): Promise<void> {
  return run(() => backend.transportSet(patch))
}

export function setPlayMode(mode: PlayMode): Promise<void> {
  return setTransport({ mode })
}

/** Selects the pattern that is edited and that plays in pattern mode. */
export function setTransportPattern(pattern: PatternId): Promise<void> {
  return setTransport({ pattern })
}

export async function seek(tick: number): Promise<void> {
  try {
    await backend.transportSeek(tick)
  } catch (error) {
    reportError(error)
  }
}
