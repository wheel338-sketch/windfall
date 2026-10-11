import { useEffect, useState, useSyncExternalStore } from "react"

import type { ChannelId } from "@/bindings"
import { auditionOff, auditionOn } from "@/features/piano-roll/audition"
import { onProjectReplaced } from "@/lib/store/replaced"

type HeldNote = { channel: ChannelId; key: number }

/** One note of one channel, as the held set names it. */
export function noteId(channel: ChannelId, key: number): string {
  return `${channel}:${key}`
}

export type HeldNotes = {
  /**
   * Starts a note for a source: a finger, or the keyboard. A source holds
   * one note at a time, and a note that several sources hold sounds once.
   */
  press(source: string, channel: ChannelId, key: number, velocity: number): void
  /** Ends the note a source holds, if it is the last one holding it. */
  release(source: string): void
  releaseAll(): void
  subscribe(listener: () => void): () => void
  /** The ids of the notes sounding now. A new set whenever it changes. */
  snapshot(): ReadonlySet<string>
}

/**
 * Pairs every audition note-on with one note-off. It remembers the channel
 * and key each source started, so a release goes to the note that sounds
 * even after the controls were pointed somewhere else.
 */
export function createHeldNotes(): HeldNotes {
  const sources = new Map<string, HeldNote>()
  const counts = new Map<string, number>()
  const listeners = new Set<() => void>()
  let held: ReadonlySet<string> = new Set()

  function publish() {
    held = new Set(counts.keys())
    for (const listener of [...listeners]) listener()
  }

  function release(source: string) {
    const note = sources.get(source)
    if (!note) return
    sources.delete(source)
    const id = noteId(note.channel, note.key)
    const count = (counts.get(id) ?? 1) - 1
    if (count > 0) {
      counts.set(id, count)
      return
    }
    counts.delete(id)
    auditionOff(note.channel, note.key)
    publish()
  }

  return {
    press(source, channel, key, velocity) {
      release(source)
      sources.set(source, { channel, key })
      const id = noteId(channel, key)
      const count = counts.get(id) ?? 0
      counts.set(id, count + 1)
      if (count > 0) return
      auditionOn(channel, key, velocity)
      publish()
    },
    release,
    releaseAll() {
      for (const source of [...sources.keys()]) release(source)
    },
    subscribe(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
    snapshot: () => held,
  }
}

/**
 * The notes one playing surface holds. Only the set of sounding notes
 * renders; which finger holds what stays out of React. Everything is
 * released when the surface goes away, when the window loses the focus (the
 * pointer-up never arrives then) and when another project is opened.
 */
export function useHeldNotes(): {
  notes: HeldNotes
  held: ReadonlySet<string>
} {
  const [notes] = useState(createHeldNotes)
  const held = useSyncExternalStore(notes.subscribe, notes.snapshot)
  useEffect(() => {
    const stop = onProjectReplaced(notes.releaseAll)
    window.addEventListener("blur", notes.releaseAll)
    return () => {
      stop()
      window.removeEventListener("blur", notes.releaseAll)
      notes.releaseAll()
    }
  }, [notes])
  return { notes, held }
}
