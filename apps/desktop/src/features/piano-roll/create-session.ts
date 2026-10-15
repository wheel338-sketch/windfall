import type { ChannelId, Note, PatternId, TimeSignature } from "@/bindings"
import { registry } from "@/lib/actions"
import { refuse } from "@/lib/errors"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { seek } from "@/lib/store/transport"

import { auditionOff, auditionOn } from "./audition"
import { Editor, type EditorContext, type EditorHost } from "./editor"
import { currentSession, PianoRollSession } from "./session"
import { snapMusicalGrid, snapTicks } from "./snap"
import { usePianoRollStore } from "./store"
import { patternMeterAt } from "./pattern-timeline"

// An empty lane has no array in the project, and the editor tells lanes
// apart by reference, so every empty lane gets this one.
const EMPTY_NOTES: readonly Note[] = []

export function currentSignature(): TimeSignature {
  const project = useProjectStore.getState().project
  const id = currentSession()?.editing?.patternId
  return project.patterns.find((pattern) => pattern.id === id)?.timeSignature ?? project.settings.timeSignature
}

/** What the editor works on, read from the project as it is right now. */
export function readContext(
  patternId: PatternId,
  channelId: ChannelId
): EditorContext | null {
  const { project } = useProjectStore.getState()
  const pattern = project.patterns.find((item) => item.id === patternId)
  if (!pattern) return null
  if (!project.channels.some((channel) => channel.id === channelId)) return null
  const lane = pattern.lanes.find((item) => item.channel === channelId)
  let elsewhereEnd = 0
  for (const other of pattern.lanes) {
    if (other === lane) continue
    for (const note of other.notes) elsewhereEnd = Math.max(elsewhereEnd, note.start + note.length)
  }
  for (const events of pattern.events ?? []) {
    for (const point of events.points) elsewhereEnd = Math.max(elsewhereEnd, point.tick)
  }
  for (const marker of pattern.timeline?.markers ?? []) elsewhereEnd = Math.max(elsewhereEnd, marker.tick)
  return {
    pattern: {
      id: pattern.id,
      lengthSteps: pattern.lengthSteps,
      signature: pattern.timeSignature ?? project.settings.timeSignature,
      timeline: pattern.timeline,
      noteCurves: pattern.noteCurves,
    },
    channel: channelId,
    notes: lane?.notes ?? EMPTY_NOTES,
    elsewhereEnd,
  }
}

/**
 * A session wired to the app: its editor dispatches to the project store,
 * reads its tool and snap from the piano roll's settings and auditions
 * through the backend.
 */
export function createSession(): PianoRollSession {
  const host: EditorHost = {
    generation: getProjectGeneration,
    dispatch: (command) => dispatch(command),
    context: () => {
      const editing = session.editing
      return editing ? readContext(editing.patternId, editing.channelId) : null
    },
    settings: (tick = 0) => {
      const state = usePianoRollStore.getState()
      const editing = session.editing
      const pattern = editing ? readContext(editing.patternId, editing.channelId)?.pattern : null
      const meter = patternMeterAt(tick, pattern?.signature ?? currentSignature(), pattern?.timeline)
      return {
        tool: state.tool,
        drum: state.drum,
        snap: snapTicks(state.snap, meter.signature),
        snapOrigin: meter.start,
        snapEnd: meter.end,
        musicalGrid: snapMusicalGrid(state.snap, pattern?.signature ?? currentSignature(), pattern?.timeline),
        lastLength: state.lastLength,
        lastVelocity: state.lastVelocity,
        articulation: state.drawArticulation,
        glideTicks: state.drawGlideTicks,
        colorGroup: state.drawColorGroup,
        pitchScale: state.snapToScale
          ? { root: state.scaleRoot, id: state.scaleId }
          : null,
      }
    },
    remember: (length, velocity) =>
      usePianoRollStore.getState().rememberNote(length, velocity),
    refuse,
    noteOn: auditionOn,
    noteOff: auditionOff,
    seek: (tick) => {
      session.setPlayhead(tick)
      void seek(tick)
    },
  }
  const editor = new Editor(host)
  const session: PianoRollSession = new PianoRollSession(editor)
  // Actions and buttons ask the store whether anything is selected.
  editor.subscribe((event) => {
    if (event === "stamp" || event === "drag") registry.invalidate()
    if (event !== "selection" && event !== "scene") return
    const selectionCount = editor.selectionCount
    if (usePianoRollStore.getState().selectionCount !== selectionCount) {
      usePianoRollStore.setState({ selectionCount })
    }
  })
  return session
}
