import { create } from "zustand"

import type { ChannelId } from "@/bindings"
import { onProjectReplaced } from "@/lib/store/replaced"

const WINDOW_MS = 20_000
const MAX_NOTES = 1_024

type HeldNote = {
  channel: ChannelId
  key: number
  velocity: number
  start: number
}
export type PlayedNote = HeldNote & { end: number }

/** Audition memory only: outside the document, persistence and undo history. */
export const useNoteLogStore = create<{
  held: readonly HeldNote[]
  notes: readonly PlayedNote[]
  dumping: boolean
}>(() => ({ held: [], notes: [], dumping: false }))

let expiry: ReturnType<typeof setTimeout> | null = null

function updateLog(held: readonly HeldNote[], notes: readonly PlayedNote[]) {
  if (expiry !== null) clearTimeout(expiry)
  expiry = null
  const now = Date.now()
  const cutoff = now - WINDOW_MS
  held = held.filter((note) => note.start >= cutoff).slice(-MAX_NOTES)
  notes = notes.filter((note) => note.start >= cutoff).slice(-MAX_NOTES)
  useNoteLogStore.setState({ held, notes })
  const oldest = Math.min(
    ...held.map((note) => note.start),
    ...notes.map((note) => note.start)
  )
  if (Number.isFinite(oldest)) {
    expiry = setTimeout(
      () => {
        expiry = null
        const log = useNoteLogStore.getState()
        updateLog(log.held, log.notes)
      },
      Math.max(1, oldest + WINDOW_MS - now + 1)
    )
  }
}

export function logAuditionOn(
  channel: ChannelId,
  key: number,
  velocity: number
) {
  const log = useNoteLogStore.getState()
  const cutoff = Date.now() - WINDOW_MS
  const held = log.held.filter((note) => note.start >= cutoff)
  // Repeated on calls for a held key do not replace its original onset.
  if (!held.some((note) => note.channel === channel && note.key === key))
    held.push({ channel, key, velocity, start: Date.now() })
  updateLog(held, log.notes)
}

export function logAuditionOff(channel: ChannelId, key: number) {
  const log = useNoteLogStore.getState()
  const held = log.held.find(
    (note) => note.channel === channel && note.key === key
  )
  updateLog(
    log.held.filter((note) => note !== held),
    held ? [...log.notes, { ...held, end: Date.now() }] : log.notes
  )
}

/**
 * A note a MIDI input played into the audition destination. The engine has
 * already sounded it; here it only joins the log, so that a dump places it
 * with the shared snap like a note played on the typing keyboard. A
 * velocity of 0 releases the key.
 */
export function logHardwareNote(
  channel: ChannelId,
  key: number,
  velocity: number
) {
  if (velocity > 0)
    logAuditionOn(channel, key, Math.min(1, Math.max(0, velocity / 127)))
  else logAuditionOff(channel, key)
}

/** Check age at read time too, in case background-tab timers were delayed. */
export function playedNotesFor(channel: ChannelId): PlayedNote[] {
  const cutoff = Date.now() - WINDOW_MS
  return useNoteLogStore
    .getState()
    .notes.filter((note) => note.channel === channel && note.start >= cutoff)
    .sort((a, b) => a.start - b.start)
}

/** Consume only the pairs actually dispatched; keep other channels and new calls. */
export function consumePlayedNotes(notes: readonly PlayedNote[]) {
  const consumed = new Set(notes)
  const log = useNoteLogStore.getState()
  updateLog(
    log.held,
    log.notes.filter((note) => !consumed.has(note))
  )
}

export function clearNoteLog() {
  updateLog([], [])
  useNoteLogStore.setState({ dumping: false })
}

onProjectReplaced(clearNoteLog)
