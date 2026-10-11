import { create } from "zustand"

import type { Command, Note } from "@/bindings"
import {
  dispatch,
  onHistoryNavigation,
  useProjectStore,
} from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { ticksPerBar, ticksToSeconds } from "@/lib/time"

import { auditionOff, auditionOn } from "./audition"
import { withExtension } from "./edit-math"
import type { EditorContext } from "./editor"
import { patternMeterAt } from "./pattern-timeline"
import {
  DEFAULT_RIFF_SETTINGS,
  RiffError,
  type RiffPlacement,
  type RiffSettings,
} from "./riff/model"
import {
  riffNoteInits,
  runRiff,
  type RiffPlace,
  type RiffResult,
} from "./riff/pipeline"
import { createRiffPlayer } from "./riff/player"
import { currentSession, type PianoRollSession } from "./session"
import { usePianoRollStore } from "./store"

const HISTORY_LABEL = "Generate riff"

/** Everything the generator reads from the piano roll, captured at open. */
export type RiffRequest = {
  context: EditorContext
  session: PianoRollSession
  revision: number
  generation: number
  /** The draw velocity, for notes when the Levels stage is off. */
  velocity: number
  /** The selected notes, for "replace the selection". */
  selection: readonly Note[]
}
export const useRiffGenerator = create<{
  request: RiffRequest | null
  pending: boolean
  auditioning: boolean
}>(() => ({
  request: null,
  pending: false,
  auditioning: false,
}))

const player = createRiffPlayer()

/** The tick after the channel's last note ends, or 0 for an empty channel. */
export function riffStart(
  notes: readonly Pick<Note, "start" | "length">[]
): number {
  return notes.reduce((end, note) => Math.max(end, note.start + note.length), 0)
}

/** The settings a newly opened generator starts from. */
export function initialRiffSettings(request: RiffRequest): RiffSettings {
  const remembered =
    usePianoRollStore.getState().riffSettings ?? DEFAULT_RIFF_SETTINGS
  return remembered.placement === "selection" && request.selection.length === 0
    ? { ...remembered, placement: "after" }
    : remembered
}

/** The first tick and the bar length there, from the pattern's own meters. */
export function riffPlace(
  request: RiffRequest,
  placement: RiffPlacement
): RiffPlace {
  if (placement === "selection" && request.selection.length === 0)
    throw new RiffError(
      "progression",
      "Select the notes to replace before opening the generator."
    )
  const origin =
    placement === "start"
      ? 0
      : placement === "selection"
        ? Math.min(...request.selection.map((note) => note.start))
        : riffStart(request.context.notes)
  const meter = patternMeterAt(
    origin,
    request.context.pattern.signature,
    request.context.pattern.timeline
  )
  return {
    origin,
    barTicks: ticksPerBar(meter.signature),
    velocity: request.velocity,
  }
}

export function generateRiff(
  request: RiffRequest,
  settings: RiffSettings
): RiffResult {
  return runRiff(settings, riffPlace(request, settings.placement))
}

export function openRiffGenerator(): void {
  const session = currentSession()
  const context = session?.editor.context
  if (
    !session ||
    !context ||
    session.editor.busy ||
    useRiffGenerator.getState().request
  )
    return
  useRiffGenerator.setState({
    request: {
      context,
      session,
      revision: useProjectStore.getState().revision,
      generation: getProjectGeneration(),
      velocity: usePianoRollStore.getState().lastVelocity,
      selection: session.editor.selectedNotes().map((note) => ({ ...note })),
    },
    pending: false,
    auditioning: false,
  })
}

/** Releases every auditioned note that is still sounding. */
export function stopRiffAudition(): void {
  player.stop()
}

export function closeRiffGenerator(): void {
  stopRiffAudition()
  useRiffGenerator.setState({
    request: null,
    pending: false,
    auditioning: false,
  })
}

export function riffRequestIsCurrent(request: RiffRequest): boolean {
  const session = currentSession()
  return (
    useRiffGenerator.getState().request === request &&
    session === request.session &&
    !session.editor.busy &&
    session.editor.context?.pattern.id === request.context.pattern.id &&
    session.editor.context.channel === request.context.channel &&
    useProjectStore.getState().revision === request.revision &&
    getProjectGeneration() === request.generation
  )
}

/**
 * Plays the riff on the request's channel at the project tempo, from its
 * first note. Nothing is written. Returns false when the request is stale.
 */
export function auditionRiff(
  request: RiffRequest,
  settings: RiffSettings
): boolean {
  if (!riffRequestIsCurrent(request)) return false
  const { notes } = generateRiff(request, settings)
  const channel = request.context.channel
  const tempo = useProjectStore.getState().project.settings.tempoBpm
  player.start(notes, ticksToSeconds(1, tempo) * 1000, {
    on: (key, velocity) => auditionOn(channel, key, velocity),
    off: (key) => auditionOff(channel, key),
    ended: () => useRiffGenerator.setState({ auditioning: false }),
  })
  useRiffGenerator.setState({ auditioning: player.playing })
  return true
}

/**
 * One command, so one undo step: `addNotes` alone, or a batch that also
 * removes the replaced selection and grows the pattern to hold the riff.
 */
export function riffCommand(
  request: RiffRequest,
  settings: RiffSettings
): Command {
  const { notes } = generateRiff(request, settings)
  const { pattern, channel } = request.context
  const add: Command = {
    type: "addNotes",
    pattern: pattern.id,
    channel,
    notes: riffNoteInits(notes),
  }
  const end = Math.max(...notes.map((note) => note.start + note.length))
  const extended = settings.extendPattern
    ? withExtension(add, HISTORY_LABEL, pattern, end)
    : add
  const commands = extended.type === "batch" ? extended.commands : [extended]
  if (settings.placement === "selection")
    commands.unshift({
      type: "removeNotes",
      pattern: pattern.id,
      channel,
      notes: request.selection.map((note) => note.id),
    })
  return commands.length === 1
    ? commands[0]
    : { type: "batch", label: HISTORY_LABEL, commands }
}

export async function applyRiff(
  request: RiffRequest,
  settings: RiffSettings
): Promise<boolean> {
  if (!riffRequestIsCurrent(request) || useRiffGenerator.getState().pending)
    return false
  const command = riffCommand(request, settings)
  stopRiffAudition()
  useRiffGenerator.setState({ pending: true })
  try {
    const result = await dispatch(command)
    if (result) usePianoRollStore.getState().setRiffSettings(settings)
    if (useRiffGenerator.getState().request === request && result)
      closeRiffGenerator()
    return result !== null
  } finally {
    if (useRiffGenerator.getState().request === request)
      useRiffGenerator.setState({ pending: false })
  }
}

onProjectReplaced(closeRiffGenerator)
onHistoryNavigation(closeRiffGenerator)
