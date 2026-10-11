import { create } from "zustand"

import type { ChopStep, Note, NoteEdge, NoteMusicalGrid, NoteTransform } from "@/bindings"
import { useProjectStore } from "@/lib/store/project"
import { onProjectReplaced } from "@/lib/store/replaced"

import { currentSession } from "./session"

export type NoteTool = NoteTransform["type"]
export const NOTE_TOOLS: {
  value: NoteTool
  label: string
  description: string
}[] = [
  {
    value: "quantize",
    label: "Quantize",
    description:
      "Move starts or ends toward an original rhythmic grid. Lengths stay fixed when moving starts.",
  },
  {
    value: "legato",
    label: "Legato",
    description:
      "Reach the next selected onset. Notes in the last chord keep their lengths.",
  },
  {
    value: "staccato",
    label: "Staccato",
    description:
      "Shorten selected lengths by a percentage, keeping at least one tick.",
  },
  {
    value: "portamento",
    label: "Portamento",
    description:
      "Tie each selected onset into the next and make every note after the first glide in from the one before. Slide notes stay slides.",
  },
  {
    value: "chop",
    label: "Chop",
    description:
      "Split at grid lines from pattern tick zero. Keep any partial first and last pieces.",
  },
  {
    value: "chopPattern",
    label: "Custom Chop",
    description:
      "Split on your repeating tick boundaries, keeping partial edges. Each step can change gate and velocity.",
  },
  {
    value: "arpeggiate",
    label: "Arpeggiate",
    description:
      "Rewrite each exact-onset chord as repeated pitch-ordered hits. Every hit keeps its source voice's velocity and pan.",
  },
  {
    value: "flam",
    label: "Flam",
    description:
      "Add one short grace hit before or after every selected note. Original notes keep all their properties.",
  },
  {
    value: "rhythmReshape",
    label: "Rhythm reshaper",
    description:
      "Remove, copy or shift notes in a chosen repeating onset cell. Other selected cells and unselected notes stay put.",
  },
  {
    value: "glue",
    label: "Glue",
    description:
      "Join touching or overlapping selected notes with the same pitch, velocity and pan.",
  },
  {
    value: "strum",
    label: "Strum",
    description:
      "Spread each selected chord with exactly matching starts, ordered by pitch. Lengths stay fixed.",
  },
  {
    value: "flipTime",
    label: "Flip time",
    description: "Mirror starts and ends across the selection's time span.",
  },
  {
    value: "flipPitch",
    label: "Flip pitch",
    description:
      "Mirror pitches between the selection's lowest and highest keys.",
  },
  {
    value: "keyRange",
    label: "Limit / transpose",
    description:
      "Transpose then place pitches in the inclusive MIDI range. Octave folding keeps pitch class where possible; otherwise it clamps.",
  },
  {
    value: "scaleVelocity",
    label: "Scale velocities",
    description:
      "Multiply selected velocities. Results stay between zero and full velocity.",
  },
]

NOTE_TOOLS.push(
  { value: "randomize", label: "Randomize / humanize", description: "Apply seeded pitch, velocity, pan, timing and length offsets to the selected notes. Expression and color groups stay with their source notes." },
  { value: "generateRandom", label: "Generate random notes", description: "Replace the selection with seeded grid hits from a chord map within its time span. Generated notes inherit source pan, expression and color groups." },
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
