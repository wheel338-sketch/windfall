import { create } from "zustand"

import type { ChopStep, Note, NoteEdge, NoteMusicalGrid, NoteTransform } from "@/bindings"
import { useProjectStore } from "@/lib/store/project"
import { onProjectReplaced } from "@/lib/store/replaced"

import { currentSession } from "./session"
import { translatedText as t } from "@/lib/i18n"

export type NoteTool = NoteTransform["type"]
export const NOTE_TOOLS: {
  value: NoteTool
  label: string
  description: string
}[] = [
  {
    value: "quantize",
    get label() { return t("Quantize") },
    get description() { return t("Move starts or ends toward an original rhythmic grid. Lengths stay fixed when moving starts.") },
  },
  {
    value: "legato",
    get label() { return t("Legato") },
    get description() { return t("Reach the next selected onset. Notes in the last chord keep their lengths.") },
  },
  {
    value: "staccato",
    get label() { return t("Staccato") },
    get description() { return t("Shorten selected lengths by a percentage, keeping at least one tick.") },
  },
  {
    value: "portamento",
    get label() { return t("Portamento") },
    get description() { return t("Tie each selected onset into the next and make every note after the first glide in from the one before. Slide notes stay slides.") },
  },
  {
    value: "chop",
    get label() { return t("Chop") },
    get description() { return t("Split at grid lines from pattern tick zero. Keep any partial first and last pieces.") },
  },
  {
    value: "chopPattern",
    get label() { return t("Custom Chop") },
    get description() { return t("Split on your repeating tick boundaries, keeping partial edges. Each step can change gate and velocity.") },
  },
  {
    value: "arpeggiate",
    get label() { return t("Arpeggiate") },
    get description() { return t("Rewrite each exact-onset chord as repeated pitch-ordered hits. Every hit keeps its source voice's velocity and pan.") },
  },
  {
    value: "flam",
    get label() { return t("Flam") },
    get description() { return t("Add one short grace hit before or after every selected note. Original notes keep all their properties.") },
  },
  {
    value: "rhythmReshape",
    get label() { return t("Rhythm reshaper") },
    get description() { return t("Remove, copy or shift notes in a chosen repeating onset cell. Other selected cells and unselected notes stay put.") },
  },
  {
    value: "glue",
    get label() { return t("Glue") },
    get description() { return t("Join touching or overlapping selected notes with the same pitch, velocity and pan.") },
  },
  {
    value: "strum",
    get label() { return t("Strum") },
    get description() { return t("Spread each selected chord with exactly matching starts, ordered by pitch. Lengths stay fixed.") },
  },
  {
    value: "flipTime",
    get label() { return t("Flip time") },
    get description() { return t("Mirror starts and ends across the selection's time span.") },
  },
  {
    value: "flipPitch",
    get label() { return t("Flip pitch") },
    get description() { return t("Mirror pitches between the selection's lowest and highest keys.") },
  },
  {
    value: "keyRange",
    get label() { return t("Limit / transpose") },
    get description() { return t("Transpose then place pitches in the inclusive MIDI range. Octave folding keeps pitch class where possible; otherwise it clamps.") },
  },
  {
    value: "scaleVelocity",
    get label() { return t("Scale velocities") },
    get description() { return t("Multiply selected velocities. Results stay between zero and full velocity.") },
  },
]

NOTE_TOOLS.push(
  { value: "randomize", get label() { return t("Randomize / humanize") }, get description() { return t("Apply seeded pitch, velocity, pan, timing and length offsets to the selected notes. Expression and color groups stay with their source notes.") } },
  { value: "generateRandom", get label() { return t("Generate random notes") }, get description() { return t("Replace the selection with seeded grid hits from a chord map within its time span. Generated notes inherit source pan, expression and color groups.") } },
)

/** Semitone classes relative to the chosen root: 0,4,7 is a major triad. */
export function parseChordMap(text: string): number | null {
  if (!text.trim() || text.length > 128) return null
  const parts = text.split(",").map((part) => part.trim())
  if (parts.length > 12 || parts.some((part) => !/^(?:[0-9]|1[01])$/.test(part))) return null
  return parts.reduce((mask, part) => mask | 1 << Number(part), 0)
}

/** Parse controls only; Rust owns every rhythmic calculation and output check. */
export function parseChopSteps(
  text: string,
  period: number
): ChopStep[] | null {
  if (
    text.length > 4096 ||
    !Number.isInteger(period) ||
    period < 1 ||
    period > 245760
  )
    return null
  const tokens = text.split(",")
  if (tokens.length < 1 || tokens.length > 64) return null
  const steps: ChopStep[] = []
  for (const token of tokens) {
    const fields = token.trim().split(":")
    if (fields.length > 3 || fields.some((part) => part.trim() === ""))
      return null
    const tick = Number(fields[0])
    const gate = fields[1] === undefined ? 1 : Number(fields[1]) / 100
    const velocity = fields[2] === undefined ? 1 : Number(fields[2]) / 100
    if (
      !Number.isInteger(tick) ||
      tick < 0 ||
      tick >= period ||
      !Number.isFinite(gate) ||
      gate <= 0 ||
      gate > 1 ||
      !Number.isFinite(velocity) ||
      velocity < 0 ||
      velocity > 4 ||
      (steps.length === 0 ? tick !== 0 : tick <= steps[steps.length - 1].tick)
    )
      return null
    steps.push({ tick, gate, velocity })
  }
  return steps
}

export type ToolRequest = {
  tool: NoteTool
  edge: NoteEdge
  grid: number
  musical: NoteMusicalGrid
  pattern: number
  channel: number
  notes: Note[]
  revision: number
}

export const useNoteTools = create<{ request: ToolRequest | null }>(() => ({
  request: null,
}))

/** Capture selection and document revision once, without changing the project. */
export function openNoteTools(tool: NoteTool, edge: NoteEdge = "start"): void {
  const session = currentSession()
  const context = session?.editor.context
  const notes = session?.editor.selectedNotes() ?? []
  if (
    !context ||
    !session ||
    session.editor.busy ||
    notes.length === 0 ||
    notes.length !== session.editor.selectionCount
  )
    return
  useNoteTools.setState({
    request: {
      tool,
      edge,
      grid: session.editor.snapInterval || 240,
      musical: session.editor.snapMusicalGrid ?? { signature: context.pattern.signature, meters: context.pattern.timeline?.meters ?? [], unit: "step", divisor: 1 },
      pattern: context.pattern.id,
      channel: context.channel,
      notes: notes.map((note) => ({ ...note })),
      revision: useProjectStore.getState().revision,
    },
  })
}

export function closeNoteTools(): void {
  useNoteTools.setState({ request: null })
}

export function requestIsCurrent(request: ToolRequest): boolean {
  if (useNoteTools.getState().request !== request) return false
  const session = currentSession()
  const context = session?.editor.context
  return (
    !!session &&
    !session.editor.busy &&
    context?.pattern.id === request.pattern &&
    context.channel === request.channel &&
    useProjectStore.getState().revision === request.revision &&
    session.editor.selectionCount === request.notes.length &&
    request.notes.every((note) => session.editor.selection.has(note.id))
  )
}

export async function applyNoteTool(
  request: ToolRequest,
  transform: NoteTransform
): Promise<boolean> {
  if (!requestIsCurrent(request)) return false
  return currentSession()!.editor.transformNotes(request.notes, transform)
}

// A new document may reuse ids and revisions. Drop the captured request.
onProjectReplaced(closeNoteTools)
