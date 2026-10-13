import type { NoteInit } from "@/bindings"
import { refuse } from "@/lib/errors"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { realtimeFrame } from "@/lib/store/realtime"
import { getProjectGeneration } from "@/lib/store/replaced"
import { useSnapStore } from "@/lib/store/snap"
import { useUiStore } from "@/lib/store/ui"
import { MAX_PATTERN_TICKS, PPQ, TICKS_PER_STEP } from "@/lib/units"

import { consumePlayedNotes, playedNotesFor, useNoteLogStore } from "./note-log"
import { patternMeterAt } from "./pattern-timeline"
import { currentSession } from "./session"
import { snapRound, snapTicks } from "./snap"
import { translatedText as t } from "@/lib/i18n"

export function canDumpPlayedNotes(): boolean {
  const editor = currentSession()?.editor
  return (
    !!editor?.context &&
    !editor.busy &&
    !useNoteLogStore.getState().dumping &&
    playedNotesFor(editor.context.channel).length > 0
  )
}

export async function dumpPlayedNotes(): Promise<void> {
  if (useUiStore.getState().centerTab !== "pianoRoll" || !canDumpPlayedNotes())
    return
  const session = currentSession()!
  const { pattern, channel } = session.editor.context!
  const played = playedNotesFor(channel)
  if (!played.length) return
  const generation = getProjectGeneration()
  const ticksPerMs =
    (useProjectStore.getState().project.settings.tempoBpm * PPQ) / 60_000
  const position = Math.max(0, realtimeFrame().tick)
  const sharedSnap = useSnapStore.getState().snap
  const notes: NoteInit[] = played.map((note) => {
    const rawStart = position + (note.start - played[0].start) * ticksPerMs
    const meter = patternMeterAt(rawStart, pattern.signature, pattern.timeline)
    return {
      key: note.key,
      velocity: note.velocity,
      pan: 0,
      start: Math.max(
        0,
        snapRound(rawStart, snapTicks(sharedSnap, meter.signature), meter.start)
      ),
      length: Math.max(
        TICKS_PER_STEP,
        Math.round((note.end - note.start) * ticksPerMs)
      ),
    }
  })
  if (notes.some((note) => note.start + note.length > MAX_PATTERN_TICKS)) {
    refuse(
      t("Could not dump played notes"),
      t("There is not enough room at the transport position.")
    )
    return
  }
  useNoteLogStore.setState({ dumping: true })
  try {
    const result = await dispatch({
      type: "addNotes",
      pattern: pattern.id,
      channel,
      notes,
    })
    if (result && generation === getProjectGeneration()) {
      consumePlayedNotes(played)
      if (currentSession() === session) session.focusGrid()
    }
  } finally {
    if (generation === getProjectGeneration())
      useNoteLogStore.setState({ dumping: false })
  }
}
