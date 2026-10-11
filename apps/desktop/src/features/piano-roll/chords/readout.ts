import type { Note } from "@/bindings"

import { detectChord, type ChordDetection } from "./detection"

type ReadoutNote = Pick<Note, "id" | "key" | "start" | "length">

/** Where the keys of the readout come from. */
export type ChordSource = "played" | "selected" | "playhead" | "none"

export const CHORD_SOURCE_LABELS: Record<ChordSource, string> = {
  played: "Played",
  selected: "Selected",
  playhead: "Playhead",
  none: "Nothing sounding",
}

export const NO_CHORD = "—"

/**
 * The keys to name, from the first source that has any: keys held live,
 * then the selected notes whatever their timing, then the notes under the
 * floored playhead (`start <= tick < start + length`). Notes and held keys
 * must belong to the open channel.
 */
export function chordReadoutKeys(
  notes: readonly ReadoutNote[],
  selection: ReadonlySet<number>,
  playhead: number | null,
  held: readonly number[] = []
): { source: ChordSource; keys: number[] } {
  if (held.length) return { source: "played", keys: [...held] }
  const selected = selection.size
    ? notes.filter((note) => selection.has(note.id))
    : []
  if (selected.length) {
    return { source: "selected", keys: selected.map((note) => note.key) }
  }
  if (typeof playhead === "number") {
    const tick = Math.floor(playhead)
    const keys: number[] = []
    for (const note of notes) {
      if (note.start <= tick && tick < note.start + note.length) {
        keys.push(note.key)
      }
    }
    if (keys.length) return { source: "playhead", keys }
  }
  return { source: "none", keys: [] }
}

/** The long chord name for the toolbar, or an em dash without keys. */
export function chordReadout(
  notes: readonly ReadoutNote[],
  selection: ReadonlySet<number>,
  playhead: number | null,
  held: readonly number[] = []
): string {
  const { keys } = chordReadoutKeys(notes, selection, playhead, held)
  return keys.length ? detectChord(keys).label : NO_CHORD
}

/** What the readout and its panel show. Replaced only when something shown changes. */
export type ChordReadoutView = {
  source: ChordSource
  label: string
  /** Null without keys. */
  detection: ChordDetection | null
}

const EMPTY_VIEW: ChordReadoutView = {
  source: "none",
  label: NO_CHORD,
  detection: null,
}

function shown(view: ChordReadoutView): string {
  const chord = view.detection
  return chord
    ? `${view.source}|${chord.symbol}|${chord.label}|${chord.bass}|${chord.pitchClasses.join(",")}`
    : view.source
}

/**
 * A reader that hands back the same view for as long as nothing it shows
 * changes: the source, the chord, its bass and its pitch classes. A
 * playhead that moves inside the same notes costs no detection, and one
 * that moves on to the same chord in another voicing gives the same object,
 * so a subscriber does not render again.
 */
export function createChordReadoutReader() {
  let input = "none:"
  let view = EMPTY_VIEW
  return (
    notes: readonly ReadoutNote[],
    selection: ReadonlySet<number>,
    playhead: number | null,
    held: readonly number[] = []
  ): ChordReadoutView => {
    const { source, keys } = chordReadoutKeys(notes, selection, playhead, held)
    const nextInput = `${source}:${keys.join(",")}`
    if (nextInput === input) return view
    input = nextInput
    let next = EMPTY_VIEW
    if (keys.length) {
      const detection = detectChord(keys)
      next = { source, label: detection.label, detection }
    }
    if (shown(next) !== shown(view)) view = next
    return view
  }
}
