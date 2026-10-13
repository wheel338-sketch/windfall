import type { Note } from "@/bindings/Note"
import { keyToRow, type TimeGridView } from "@/lib/canvas"

import { HIGHEST_KEY, SONG_TICKS, TICKS_PER_BAR } from "./generate-notes"
import { ROW_COUNT, type BenchModel } from "./model"
import { translatedText as t } from "@/lib/i18n"

export interface ScenarioContext {
  readonly view: TimeGridView
  readonly model: BenchModel
}

export interface Scenario {
  readonly name: string
  readonly description: string
  setup(context: ScenarioContext): void
  /** `progress` runs 0 to 1 over the measured time. */
  frame(context: ScenarioContext, progress: number, elapsedMs: number): void
  teardown?(context: ScenarioContext): void
}

const TAU = Math.PI * 2
const DRAG_SELECTION = 1000
const TOP_PIANO_ROW = keyToRow(HIGHEST_KEY, ROW_COUNT)
const PIANO_KEYS = 88
// 140 BPM in ticks per millisecond.
const PLAYHEAD_TICKS_PER_MS = (140 * 960) / 60_000

const SWEEP_TURN = 0.55

/**
 * 0 to 1 and back to 0 over one unit of progress, with soft turnarounds.
 * The way back is faster than the way out. A symmetric sweep gives two
 * frames either side of the turn the same value, and a frame that changes
 * nothing draws nothing, which would flatter the averages.
 */
function sweep(progress: number): number {
  const p = progress - Math.floor(progress)
  const x = p < SWEEP_TURN ? p / SWEEP_TURN : (1 - p) / (1 - SWEEP_TURN)
  return 0.5 - 0.5 * Math.cos(Math.PI * x)
}

/** Eight bars across the view and 16 px rows around the middle of the keyboard. */
function workingView(view: TimeGridView, scrollTick: number): void {
  const { width, height } = view.viewport
  const rowHeight = 16
  view.setViewport({
    ...view.viewport,
    pxPerTick: width / (8 * TICKS_PER_BAR),
    rowHeight,
    scrollTick,
    scrollRow: keyToRow(64, ROW_COUNT) - height / rowHeight / 2,
  })
}

/** 32 bars across and every piano key visible, so 1,000 notes fit on screen. */
function wideView(view: TimeGridView): void {
  const { width, height } = view.viewport
  view.setViewport({
    ...view.viewport,
    pxPerTick: width / (32 * TICKS_PER_BAR),
    rowHeight: height / PIANO_KEYS,
    scrollTick: 40 * TICKS_PER_BAR,
    scrollRow: TOP_PIANO_ROW,
  })
}

function selectForDrag(context: ScenarioContext): Set<number> {
  const { view, model } = context
  const batch = model.items.batch
  const from = view.viewport.scrollTick + 2 * TICKS_PER_BAR
  const indices: number[] = []
  for (let i = 0; i < batch.count && indices.length < DRAG_SELECTION; i++) {
    if (batch.start(i) >= from) indices.push(i)
  }
  batch.setSelection(indices)
  view.invalidate("base")
  return new Set(indices.map((i) => batch.ids[i]))
}

/**
 * A drag path that keeps moving in both axes. Time is not snapped to the
 * grid (a real drag with snapping off), so every frame moves the notes.
 */
function dragOffset(progress: number): { ticks: number; rows: number } {
  return {
    ticks: Math.round(Math.sin(TAU * 2 * progress + 0.3) * 4 * TICKS_PER_BAR),
    rows: Math.round(Math.sin(TAU * 3 * progress + 1.1) * 6),
  }
}

/** Fresh scenarios. Some keep state between setup and teardown. */
export function createScenarios(): Scenario[] {
  let originalNotes: Note[] = []
  let editIds = new Set<number>()
  let playheadStart = 0

  const scroll: Scenario = {
    name: "scroll",
    description:
      t("Continuous horizontal scroll through the whole song and back. 8 bars across the view, 16 px rows."),
    setup({ view }) {
      workingView(view, 0)
    },
    frame({ view }, progress) {
      const { width, pxPerTick } = view.viewport
      const range = SONG_TICKS - width / pxPerTick
      view.setViewport({
        ...view.viewport,
        scrollTick: sweep(progress) * range,
      })
    },
  }

  const zoom: Scenario = {
    name: "zoom",
    description:
      t("Zoom sweep around the middle of the song, from all 200 bars across the view to 2 bars and back."),
    setup({ view }) {
      workingView(view, 0)
    },
    frame({ view }, progress) {
      const { width } = view.viewport
      const zoomedOut = width / SONG_TICKS
      const zoomedIn = width / (2 * TICKS_PER_BAR)
      const pxPerTick = zoomedOut * (zoomedIn / zoomedOut) ** sweep(progress)
      view.setViewport({
        ...view.viewport,
        pxPerTick,
        scrollTick: SONG_TICKS / 2 - width / 2 / pxPerTick,
      })
    },
  }

  const drag: Scenario = {
    name: "drag",
    description:
      t("Dragging 1,000 selected notes in time and pitch. The move is a drag offset on the view; no note data changes until the drop."),
    setup(context) {
      wideView(context.view)
      selectForDrag(context)
    },
    frame({ view }, progress) {
      const offset = dragOffset(progress)
      view.setDragOffset(offset.ticks, offset.rows)
    },
    teardown({ view, model }) {
      view.setDragOffset(0, 0)
      model.items.batch.clearSelection()
      view.invalidate("base")
    },
  }

  const editRebuild: Scenario = {
    name: "edit-rebuild",
    description:
      t("Worst case for an edit: every frame 1,000 notes change in the model, the whole batch is rebuilt from the Note array (colors, sort, index) and uploaded again."),
    setup(context) {
      wideView(context.view)
      originalNotes = context.model.notes
      editIds = selectForDrag(context)
    },
    frame({ model }, progress) {
      const offset = dragOffset(progress)
      model.notes = originalNotes.map((note) =>
        editIds.has(note.id)
          ? {
              ...note,
              start: note.start + offset.ticks,
              key: note.key - offset.rows,
            }
          : note
      )
      model.rebuild(editIds)
    },
    teardown({ model }) {
      model.notes = originalNotes
      model.rebuild(new Set())
      originalNotes = []
      editIds = new Set()
    },
  }

  const playhead: Scenario = {
    name: "playhead",
    description:
      t("Only the playhead moves, at 140 BPM. The grid and the notes are not redrawn."),
    setup({ view }) {
      workingView(view, 40 * TICKS_PER_BAR)
      playheadStart = view.viewport.scrollTick
    },
    frame({ view }, _progress, elapsedMs) {
      const span = view.viewport.width / view.viewport.pxPerTick
      view.setPlayhead(
        playheadStart + ((elapsedMs * PLAYHEAD_TICKS_PER_MS) % span)
      )
    },
    teardown({ view }) {
      view.setPlayhead(null)
    },
  }

  const overview: Scenario = {
    name: "overview",
    description:
      t("Every note on screen at once: the whole song across the view and all 88 keys, with a small zoom and scroll wobble so each frame is a full redraw."),
    setup({ view }) {
      const { width, height } = view.viewport
      view.setViewport({
        ...view.viewport,
        pxPerTick: width / SONG_TICKS,
        rowHeight: height / (PIANO_KEYS + 2),
        scrollTick: 0,
        scrollRow: TOP_PIANO_ROW - 1,
      })
    },
    frame({ view }, progress) {
      const { width } = view.viewport
      view.setViewport({
        ...view.viewport,
        // Zooming out only, so no note ever leaves the right edge.
        pxPerTick: (width / SONG_TICKS) * (1 - 0.04 * sweep(progress * 4)),
        scrollTick: 0,
        scrollRow: TOP_PIANO_ROW - 1 + Math.sin(TAU * 2 * progress + 0.7) * 0.9,
      })
    },
  }

  return [scroll, zoom, drag, editRebuild, playhead, overview]
}
