import type { NoteInit } from "@/bindings"
import { MAX_PATTERN_TICKS } from "@/lib/units"

import {
  MAX_RIFF_NOTES,
  RIFF_ARP_RATES,
  RIFF_STAGES,
  RiffError,
  validateRiffSettings,
  type RiffContext,
  type RiffNote,
  type RiffSettings,
  type RiffStageId,
} from "./model"
import { createRiffRng } from "./rng"
import {
  arpeggioStage,
  articulationStage,
  bySpan,
  chordsStage,
  fitStage,
  grooveStage,
  levelsStage,
  mirrorStage,
  progressionStage,
} from "./stages"
import { chordName } from "./theory"

/** Where a riff goes and what it inherits from the piano roll. */
export type RiffPlace = {
  /** First tick of the riff. */
  origin: number
  /** Bar length of the meter at `origin`. */
  barTicks: number
  /** Velocity for notes when the Levels stage is off, 0 to 1. */
  velocity: number
}

export type RiffResult = {
  /** Sorted by start, then key. */
  notes: RiffNote[]
  context: RiffContext
  /** Notes leaving each stage, or null for a stage that was switched off. */
  counts: Record<RiffStageId, number | null>
  /** One name per chord of the progression, before voicing. */
  chords: string[]
  /** The requested span: `origin` to `origin + bars`. */
  start: number
  end: number
}

/**
 * Runs the eight stages in order. Each random stage draws from its own
 * stream of the one seed, so a stage's random settings never disturb
 * another's. Throws a `RiffError` instead of returning a shortened riff.
 */
export function runRiff(settings: RiffSettings, place: RiffPlace): RiffResult {
  validateRiffSettings(settings)
  if (
    !Number.isInteger(place.origin) ||
    place.origin < 0 ||
    !Number.isInteger(place.barTicks) ||
    place.barTicks < 1 ||
    !Number.isFinite(place.velocity) ||
    place.velocity < 0 ||
    place.velocity > 1
  )
    throw new RiffError(
      "progression",
      "The current meter or draw velocity is invalid."
    )
  const end = place.origin + settings.progression.bars * place.barTicks
  if (end > MAX_PATTERN_TICKS)
    throw new RiffError(
      "progression",
      "There is not enough room for this riff before the end of the longest pattern. Shorten it or place it earlier."
    )

  const context: RiffContext = {
    origin: place.origin,
    barTicks: place.barTicks,
    velocity: place.velocity,
    scale: {
      root: settings.progression.root,
      id: settings.progression.scale,
    },
    grid: settings.arpeggio.enabled
      ? RIFF_ARP_RATES.find((rate) => rate.value === settings.arpeggio.rate)!
          .ticks
      : 0,
  }
  const rng = createRiffRng(settings.seed)
  const counts = Object.fromEntries(
    RIFF_STAGES.map((stage) => [stage.id, null])
  ) as Record<RiffStageId, number | null>

  let notes = progressionStage(
    [],
    settings.progression,
    context,
    rng.fork("progression")
  )
  counts.progression = notes.length
  const chords = bySpan(notes).map((chord) =>
    chordName(chord.map((note) => note.key))
  )
  if (settings.chords.enabled) {
    notes = chordsStage(notes, settings.chords, context, rng.fork("chords"))
    counts.chords = notes.length
  }
  if (settings.arpeggio.enabled) {
    notes = arpeggioStage(
      notes,
      settings.arpeggio,
      context,
      rng.fork("arpeggio")
    )
    counts.arpeggio = notes.length
  }
  if (settings.mirror.enabled) {
    notes = mirrorStage(notes, settings.mirror, context, rng.fork("mirror"))
    counts.mirror = notes.length
  }
  if (settings.levels.enabled) {
    notes = levelsStage(notes, settings.levels, context, rng.fork("levels"))
    counts.levels = notes.length
  }
  if (settings.articulation.enabled) {
    notes = articulationStage(
      notes,
      settings.articulation,
      context,
      rng.fork("articulation")
    )
    counts.articulation = notes.length
  }
  if (settings.groove.enabled) {
    notes = grooveStage(notes, settings.groove, context, rng.fork("groove"))
    counts.groove = notes.length
  }
  // Always runs: switched off it still folds keys into MIDI 0–127.
  notes = fitStage(notes, settings.fit, context, rng.fork("fit"))
  if (settings.fit.enabled) counts.fit = notes.length

  if (notes.length === 0)
    throw new RiffError("progression", "These settings leave no notes.")
  if (notes.length > MAX_RIFF_NOTES)
    throw new RiffError(
      "fit",
      `This riff has ${notes.length} notes and one riff can add at most ${MAX_RIFF_NOTES}. Shorten it or slow the arpeggio.`
    )
  if (notes.some((note) => note.start + note.length > MAX_PATTERN_TICKS))
    throw new RiffError(
      "fit",
      "A note would run past the end of the longest pattern. Shorten the riff or its notes."
    )
  return {
    notes: [...notes].sort((a, b) => a.start - b.start || a.key - b.key),
    context,
    counts,
    chords,
    start: place.origin,
    end,
  }
}

/** Notes as the `addNotes` command takes them. */
export function riffNoteInits(notes: readonly RiffNote[]): NoteInit[] {
  return notes.map((note) => {
    const init: NoteInit = {
      start: note.start,
      length: note.length,
      key: note.key,
      velocity: note.velocity,
      pan: note.pan,
    }
    // Neutral values are left out so the notes are plain ones.
    if (note.release !== 0.5 || note.modX !== 0.5 || note.modY !== 0.5)
      init.expression = {
        release: note.release,
        finePitchCents: 0,
        modulationX: note.modX,
        modulationY: note.modY,
      }
    return init
  })
}
