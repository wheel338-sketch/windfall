import { create } from "zustand"

import type { Command, Note, NoteInit } from "@/bindings"
import {
  dispatch,
  onHistoryNavigation,
  useProjectStore,
} from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { ticksPerBar } from "@/lib/time"
import { DEFAULT_KEY, MAX_PATTERN_TICKS } from "@/lib/units"

import type { EditorContext } from "./editor"
import { patternMeterAt } from "./pattern-timeline"
import { currentSession, type PianoRollSession } from "./session"
import { usePianoRollStore } from "./store"
import { translatedText as t } from "@/lib/i18n"

export const PROGRESSION_MODES = [
  { value: "major", get label() { return t("Major") } },
  { value: "minor", get label() { return t("Natural minor") } },
] as const
export const PROGRESSION_MOODS = [
  { value: "bright", get label() { return t("Bright") } },
  { value: "calm", get label() { return t("Calm") } },
  { value: "tense", get label() { return t("Tense") } },
] as const

export type ProgressionSettings = {
  root: number
  mode: (typeof PROGRESSION_MODES)[number]["value"]
  mood: (typeof PROGRESSION_MOODS)[number]["value"]
  bars: number
  seed: number
}
type Timing = { barTicks: number; velocity: number }

const INTERVALS = {
  major: [0, 2, 4, 5, 7, 9, 11],
  minor: [0, 2, 3, 5, 7, 8, 10],
} as const
const CYCLES = {
  major: {
    bright: [0, 4, 5, 3],
    calm: [0, 5, 3, 4],
    tense: [6, 4, 0, 3],
  },
  minor: {
    bright: [0, 5, 2, 6],
    calm: [0, 5, 3, 4],
    tense: [0, 6, 5, 4],
  },
} as const
const COMMON_TIME = { numerator: 4, denominator: 4 }

export function progressionStart(
  notes: readonly Pick<Note, "start" | "length">[]
): number {
  return notes.reduce((end, note) => Math.max(end, note.start + note.length), 0)
}

/** Diatonic triads from a fixed mood cycle; leaves the source notes untouched. */
export function generateProgressionNotes(
  source: readonly Pick<Note, "key" | "start" | "length">[],
  settings: ProgressionSettings,
  timing: Timing
): NoteInit[] {
  if (
    !Number.isInteger(settings.root) ||
    settings.root < 0 ||
    settings.root > 11 ||
    !PROGRESSION_MODES.some((item) => item.value === settings.mode) ||
    !PROGRESSION_MOODS.some((item) => item.value === settings.mood) ||
    !Number.isInteger(settings.bars) ||
    settings.bars < 2 ||
    settings.bars > 8 ||
    !Number.isSafeInteger(settings.seed) ||
    settings.seed < 0
  )
    throw new Error(
      t("Choose a root, mode, mood, 2–8 bars and a non-negative integer seed.")
    )
  if (
    !Number.isInteger(timing.barTicks) ||
    timing.barTicks < 1 ||
    !Number.isFinite(timing.velocity) ||
    timing.velocity < 0 ||
    timing.velocity > 1
  )
    throw new Error(t("The current meter or draw velocity is invalid."))

  const start = progressionStart(source)
  if (start + settings.bars * timing.barTicks > MAX_PATTERN_TICKS)
    throw new Error(
      t("There is not enough room after the channel's notes for this progression.")
    )

  let low = 127
  let high = 0
  for (const note of source) {
    low = Math.min(low, note.key)
    high = Math.max(high, note.key)
  }
  const center = source.length ? Math.round((low + high) / 2) : DEFAULT_KEY
  // Chord roots occupy a centered octave. Leave seven semitones above it
  // for the fifth so entire triads stay inside MIDI 24–96 without clipping.
  const bottom = Math.max(24, Math.min(78, center - 6))
  const intervals = INTERVALS[settings.mode]
  const cycle = CYCLES[settings.mode][settings.mood]
  const rotation = settings.seed % cycle.length
  const scaleStep = (step: number) =>
    intervals[step % intervals.length] +
    12 * Math.floor(step / intervals.length)
  const notes: NoteInit[] = []
  for (let bar = 0; bar < settings.bars; bar++) {
    const degree = cycle[(rotation + bar) % cycle.length]
    const pitchClass = (settings.root + intervals[degree]) % 12
    const root = bottom + ((pitchClass - (bottom % 12) + 12) % 12)
    for (const step of [degree, degree + 2, degree + 4]) {
      notes.push({
        start: start + bar * timing.barTicks,
        length: timing.barTicks,
        key: root + scaleStep(step) - scaleStep(degree),
        velocity: timing.velocity,
        pan: 0,
      })
    }
  }
  return notes
}

export type ProgressionRequest = Timing & {
  context: EditorContext
  session: PianoRollSession
  revision: number
  generation: number
}
export const useProgressionGenerator = create<{
  request: ProgressionRequest | null
  pending: boolean
}>(() => ({ request: null, pending: false }))

export function openProgressionGenerator(): void {
  const session = currentSession()
  const context = session?.editor.context
  if (
    !session ||
    !context ||
    session.editor.busy ||
    useProgressionGenerator.getState().request
  )
    return
  const state = useProjectStore.getState()
  const pattern = state.project.patterns.find(
    (item) => item.id === context.pattern.id
  )
  const meter = patternMeterAt(
    progressionStart(context.notes),
    pattern?.timeSignature ?? COMMON_TIME,
    context.pattern.timeline
  )
  useProgressionGenerator.setState({
    request: {
      context,
      session,
      revision: state.revision,
      generation: getProjectGeneration(),
      barTicks: ticksPerBar(meter.signature),
      velocity: usePianoRollStore.getState().lastVelocity,
    },
    pending: false,
  })
}

export function closeProgressionGenerator(): void {
  useProgressionGenerator.setState({ request: null, pending: false })
}

export function progressionRequestIsCurrent(
  request: ProgressionRequest
): boolean {
  const session = currentSession()
  return (
    useProgressionGenerator.getState().request === request &&
    session === request.session &&
    !session.editor.busy &&
    session.editor.context?.pattern.id === request.context.pattern.id &&
    session.editor.context.channel === request.context.channel &&
    useProjectStore.getState().revision === request.revision &&
    getProjectGeneration() === request.generation
  )
}

export function progressionCommand(
  request: ProgressionRequest,
  settings: ProgressionSettings
): Command {
  return {
    type: "addNotes",
    pattern: request.context.pattern.id,
    channel: request.context.channel,
    notes: generateProgressionNotes(request.context.notes, settings, request),
  }
}

export async function applyProgression(
  request: ProgressionRequest,
  settings: ProgressionSettings
): Promise<boolean> {
  if (
    !progressionRequestIsCurrent(request) ||
    useProgressionGenerator.getState().pending
  )
    return false
  const command = progressionCommand(request, settings)
  useProgressionGenerator.setState({ pending: true })
  try {
    const result = await dispatch(command)
    if (useProgressionGenerator.getState().request === request && result)
      closeProgressionGenerator()
    return result !== null
  } finally {
    if (useProgressionGenerator.getState().request === request)
      useProgressionGenerator.setState({ pending: false })
  }
}

onProjectReplaced(closeProgressionGenerator)
onHistoryNavigation(closeProgressionGenerator)
