import type { Channel, Note, Project } from "@/bindings"
import { registry, type Action } from "@/lib/actions"
import type { IndexedBatch, Marquee, Viewport } from "@/lib/canvas"
import { useProjectStore } from "@/lib/store/project"
import { selectedPatternId } from "@/lib/store/selectors"
import { useTransportStore } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { settle, startTestApp } from "@/test/harness"

import { registerPianoRollActions } from "./actions"
import { writeClipboard } from "./clipboard"
import { createSession, readContext } from "./create-session"
import type { EditorSurface, PointerInput } from "./editor"
import type { PressButton } from "./intents"
import { forgetSavedViews, setCurrentSession } from "./session"
import { usePianoRollStore } from "./store"
import { message, translatedText as t } from "@/lib/i18n"

/** A step is 12 px wide and a row 16 px tall; C5 sits at y 160 to 176. */
export const TEST_VIEWPORT: Viewport = {
  width: 800,
  height: 320,
  dpr: 1,
  scrollTick: 0,
  scrollRow: 57,
  pxPerTick: 0.05,
  rowHeight: 16,
}

/** Stands in for the canvas view and records what the editor asks of it. */
export class FakeSurface implements EditorSurface {
  viewport: Viewport = TEST_VIEWPORT
  items: IndexedBatch | null = null
  dragOffset = { ticks: 0, rows: 0 }
  dragResize = { start: 0, end: 0 }
  marquee: Marquee | null = null
  invalidations = 0

  setViewport(viewport: Viewport) {
    this.viewport = viewport
  }

  setItems(items: IndexedBatch | null) {
    this.items = items
  }
  setDragOffset(ticks: number, rows: number) {
    this.dragOffset = { ticks, rows }
  }
  setDragResize(start: number, end: number) {
    this.dragResize = { start, end }
  }
  setMarquee(marquee: Marquee | null) {
    this.marquee = marquee
  }
  invalidate() {
    this.invalidations += 1
  }
}

export type Modifiers = Partial<Pick<PointerInput, "shift" | "ctrl" | "alt">>

/** The pointer over a tick and a key. `inRow` runs 0 to 1 down the row. */
export function at(
  tick: number,
  key: number,
  modifiers: Modifiers = {},
  inRow = 0.5
): PointerInput {
  return {
    x: (tick - TEST_VIEWPORT.scrollTick) * TEST_VIEWPORT.pxPerTick,
    y: (127 - key - TEST_VIEWPORT.scrollRow + inRow) * TEST_VIEWPORT.rowHeight,
    shift: false,
    ctrl: false,
    alt: false,
    ...modifiers,
  }
}

export const project = () => useProjectStore.getState().project
export const history = () => useProjectStore.getState().history
export const undoSteps = () => history().cursor

export function channel(name: string): Channel {
  const found = project().channels.find((item) => item.name === name)
  if (!found) throw new Error(message("No channel called {name}", { name }))
  return found
}

export function currentPattern() {
  const id = selectedPatternId(project(), useTransportStore.getState().pattern)
  const pattern = project().patterns.find((item) => item.id === id)
  if (!pattern) throw new Error(t("No pattern"))
  return pattern
}

export function notesOf(name: string): Note[] {
  const id = channel(name).id
  return currentPattern().lanes.find((lane) => lane.channel === id)?.notes ?? []
}

/** Notes as `start:key:length`, which reads well in a failed assertion. */
export function brief(notes: readonly Note[]): string[] {
  return notes.map((note) => `${note.start}:${note.key}:${note.length}`)
}

/**
 * The test app with a piano roll session open on one channel of the
 * current pattern, driven through a fake surface instead of a canvas.
 */
export async function startRoll(
  options: {
    project?: Project
    /** The channel to open. Null opens none, for a project without channels. */
    channel?: string | null
    /** Actions of another panel, registered first the way the app does. */
    earlier?: Action[]
  } = {}
) {
  const app = await startTestApp({ project: options.project })
  const unregisterEarlier = registry.register(options.earlier ?? [])
  const unregister = registerPianoRollActions()
  usePianoRollStore.setState(usePianoRollStore.getInitialState(), true)
  // Piano editing fixtures use step cells regardless of the shared session default.
  usePianoRollStore.getState().setSnap("step")
  writeClipboard(null)
  forgetSavedViews()
  useUiStore.getState().showCenterTab("pianoRoll")

  const session = createSession()
  const surface = new FakeSurface()
  const { editor } = session
  editor.attach(surface)
  setCurrentSession(session)

  /** Points the session at a channel, as the panel does when it renders. */
  function show(name: string) {
    const id = channel(name).id
    useUiStore.getState().selectChannel(id)
    session.setEditing(currentPattern().id, id)
    editor.setContext(readContext(currentPattern().id, id))
  }
  // The demo song's first channel, its kick, unless the test names another.
  if (options.channel !== null) show(options.channel ?? project().channels[0].name)
  // The panel passes every change of the lane on to the editor.
  const unfollow = useProjectStore.subscribe(() => {
    const editing = session.editing
    if (editing) {
      editor.setContext(readContext(editing.patternId, editing.channelId))
    }
  })

  async function click(
    input: PointerInput,
    button: PressButton = "left"
  ): Promise<void> {
    editor.pointerDown(input, button)
    editor.pointerUp(input)
    await settle()
  }

  /** Presses at `from`, moves through `path` and releases at its last point. */
  async function drag(
    from: PointerInput,
    path: PointerInput | PointerInput[],
    button: PressButton = "left"
  ): Promise<void> {
    const points = Array.isArray(path) ? path : [path]
    editor.pointerDown(from, button)
    for (const point of points) editor.pointerMove(point)
    editor.pointerUp(points[points.length - 1])
    await settle()
  }

  return {
    backend: app.backend,
    session,
    editor,
    surface,
    show,
    click,
    drag,
    stop() {
      unfollow()
      setCurrentSession(null)
      editor.dispose()
      session.dispose()
      unregister()
      unregisterEarlier()
      app.stop()
    },
  }
}
