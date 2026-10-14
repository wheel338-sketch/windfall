import type { NoteInit } from "@/bindings"

import { MAX_PATTERN_TICKS } from "./edit-math"
import { SCALES } from "./scales"
import { message, translatedText as t } from "@/lib/i18n"

export type Stamp = {
  id: string
  label: string
  intervals: readonly number[]
  layout: "chord" | "ascending" | "descending"
}

export const CHORD_STAMPS: readonly Stamp[] = [
  { id: "major", get label() { return t("Major triad") }, intervals: [0, 4, 7], layout: "chord" },
  { id: "minor", get label() { return t("Minor triad") }, intervals: [0, 3, 7], layout: "chord" },
  {
    id: "diminished",
    get label() { return t("Diminished triad") },
    intervals: [0, 3, 6],
    layout: "chord",
  },
  {
    id: "augmented",
    get label() { return t("Augmented triad") },
    intervals: [0, 4, 8],
    layout: "chord",
  },
  {
    id: "sus2",
    get label() { return t("Suspended second") },
    intervals: [0, 2, 7],
    layout: "chord",
  },
  {
    id: "sus4",
    get label() { return t("Suspended fourth") },
    intervals: [0, 5, 7],
    layout: "chord",
  },
  { id: "power", get label() { return t("Power chord") }, intervals: [0, 7, 12], layout: "chord" },
  {
    id: "major6",
    get label() { return t("Major sixth") },
    intervals: [0, 4, 7, 9],
    layout: "chord",
  },
  {
    id: "minor6",
    get label() { return t("Minor sixth") },
    intervals: [0, 3, 7, 9],
    layout: "chord",
  },
  {
    id: "dominant7",
    get label() { return t("Dominant seventh") },
    intervals: [0, 4, 7, 10],
    layout: "chord",
  },
  {
    id: "major7",
    get label() { return t("Major seventh") },
    intervals: [0, 4, 7, 11],
    layout: "chord",
  },
  {
    id: "minor7",
    get label() { return t("Minor seventh") },
    intervals: [0, 3, 7, 10],
    layout: "chord",
  },
  {
    id: "half-diminished7",
    get label() { return t("Half-diminished seventh") },
    intervals: [0, 3, 6, 10],
    layout: "chord",
  },
  {
    id: "diminished7",
    get label() { return t("Diminished seventh") },
    intervals: [0, 3, 6, 9],
    layout: "chord",
  },
  {
    id: "major9",
    get label() { return t("Major ninth") },
    intervals: [0, 4, 7, 11, 14],
    layout: "chord",
  },
  {
    id: "minor9",
    get label() { return t("Minor ninth") },
    intervals: [0, 3, 7, 10, 14],
    layout: "chord",
  },
]

export const SCALE_STAMPS: readonly Stamp[] = SCALES.flatMap((scale) =>
  (["ascending", "descending"] as const).map((layout) => ({
    id: `${scale.id}-${layout}`,
    get label() {
      return layout === "ascending"
        ? message("{scale} scale ascending", { scale: scale.label })
        : message("{scale} scale descending", { scale: scale.label })
    },
    intervals: [...scale.intervals, 12],
    layout,
  }))
)

export type StampPlacement =
  { notes: NoteInit[]; error: null } | { notes: []; error: string }

/** A complete pattern, or no notes. Descending patterns end on the clicked root. */
export function placeStamp(
  stamp: Stamp,
  tick: number,
  key: number,
  length: number,
  velocity: number
): StampPlacement {
  const fail = (error: string): StampPlacement => ({ notes: [], error })
  if (
    ![tick, key, length, velocity].every(Number.isFinite) ||
    !Number.isInteger(tick) ||
    tick < 0 ||
    !Number.isInteger(length) ||
    length < 1 ||
    !Number.isInteger(key) ||
    velocity < 0 ||
    velocity > 1
  ) {
    return fail("The position, length or velocity is invalid.")
  }
  if (
    !stamp.intervals.length ||
    stamp.intervals.some((interval) => !Number.isInteger(interval))
  )
    return fail("The stamp has no valid pattern.")
  const intervals =
    stamp.layout === "descending"
      ? [...stamp.intervals].reverse()
      : stamp.intervals
  const notes: NoteInit[] = intervals.map((interval, index) => ({
    start: tick + (stamp.layout === "chord" ? 0 : index * length),
    key: key + interval,
    length,
    velocity,
    pan: 0,
  }))
  if (notes.some((note) => note.key < 0 || note.key > 127))
    return fail(
      "The whole stamp must fit inside MIDI keys 0–127. Choose a lower root."
    )
  if (
    notes.some(
      (note) =>
        !Number.isSafeInteger(note.start + note.length) ||
        note.start + note.length > MAX_PATTERN_TICKS
    )
  )
    return fail(
      "The whole stamp must fit inside the maximum pattern length. Choose an earlier position or shorter notes."
    )
  return { notes, error: null }
}
