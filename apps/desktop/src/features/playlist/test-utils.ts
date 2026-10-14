import { historyLabelText } from "@/lib/i18n"
import type { Project } from "@/bindings"
import {
  clampViewport,
  deriveGridTheme,
  hitTestPoint,
  rgba,
  type GridTheme,
  type Hit,
  type HitOptions,
  type IndexedBatch,
  type Marquee,
  type OverlayPainter,
  type RowStyle,
  type TimeGridSpec,
  type Viewport,
  type ViewportLimits,
} from "@/lib/canvas"
import { useProjectStore } from "@/lib/store/project"
import { usePromptStore } from "@/lib/store/prompts"
import { useUiStore } from "@/lib/store/ui"
import { settle, startTestApp } from "@/test/harness"

import { registerPlaylistActions } from "./actions"
import { GridMetrics, PLAYLIST_LIMITS, resetSavedView } from "./metrics"
import { PlaylistSession, type PointerInput } from "./session"
import { usePlaylistStore } from "./store"
import type { GridSurface } from "./surface"

/* Helpers the playlist's tests share. Not part of the app. */

export const BAR = 3840
export const BEAT = 960
export const STEP = 240

/** The test grid: a bar is 96 pixels wide and a row 20 pixels tall. */
export const PX_PER_TICK = 0.025
export const ROW_HEIGHT = 20

/**
 * Stands in for the canvas. It keeps what it is told and answers hit tests
 * from the real batch, so everything but the drawing is the real thing.
 */
export class FakeSurface implements GridSurface {
  viewport: Viewport = {
    width: 960,
    height: 400,
    dpr: 1,
    scrollTick: 0,
    scrollRow: 0,
    pxPerTick: PX_PER_TICK,
    rowHeight: ROW_HEIGHT,
  }
  limits: ViewportLimits = PLAYLIST_LIMITS
  theme: GridTheme = deriveGridTheme({
    background: rgba(20, 20, 22),
    foreground: rgba(240, 240, 240),
    mutedForeground: rgba(160, 160, 160),
    brand: rgba(230, 60, 140),
    playhead: rgba(90, 150, 240),
    gridLine: rgba(255, 255, 255, 18),
    gridLineStrong: rgba(255, 255, 255, 51),
  })
  items: IndexedBatch | null = null
  rows: RowStyle | null = null
  timeGrid: TimeGridSpec | null = null
  drag = { ticks: 0, rows: 0 }
  resize = { start: 0, end: 0, minLength: 0 }
  marquee: Marquee | null = null
  playhead: number | null = null
  /** How many times a new batch was handed over. */
  rebuilds = 0
  private viewportListeners: ((viewport: Viewport) => void)[] = []

  setViewport(next: Viewport): void {
    this.viewport = clampViewport(next, this.limits)
    for (const listener of this.viewportListeners) listener(this.viewport)
  }

  setLimits(limits: Partial<ViewportLimits>): void {
    this.limits = { ...this.limits, ...limits }
    this.setViewport(this.viewport)
  }

  setRows(rows: RowStyle): void {
    this.rows = rows
  }

  setTimeGrid(spec: TimeGridSpec): void {
    this.timeGrid = spec
  }

  setItems(items: IndexedBatch | null): void {
    this.items = items
    this.rebuilds++
  }

  setDragOffset(ticks: number, rows: number): void {
    this.drag = { ticks, rows }
  }

  setDragResize(start: number, end: number, minLength = 1): void {
    this.resize = { start, end, minLength }
  }

  setMarquee(marquee: Marquee | null): void {
    this.marquee = marquee
  }

  setPlayhead(tick: number | null): void {
    this.playhead = tick
  }

  addOverlayPainter(painter: OverlayPainter): () => void {
    void painter
    return () => {}
  }

  onThemeChange(): () => void {
    return () => {}
  }

  onViewportChange(listener: (viewport: Viewport) => void): () => void {
    this.viewportListeners.push(listener)
    return () => {
      this.viewportListeners = this.viewportListeners.filter(
        (item) => item !== listener
      )
    }
  }

  invalidate(): void {}

  hitTest(x: number, y: number, options?: HitOptions): Hit | null {
    return this.items
      ? hitTestPoint(this.viewport, this.items, x, y, options)
      : null
  }
}

/** The test app with the playlist's actions and a clean playlist state. */
export async function startPlaylist(options: { project?: Project } = {}) {
  const app = await startTestApp(options)
  const unregister = registerPlaylistActions()
  usePlaylistStore.setState(usePlaylistStore.getInitialState(), true)
  resetSavedView()
  useUiStore.getState().showCenterTab("playlist")
  return {
    backend: app.backend,
    stop() {
      unregister()
      app.stop()
    },
  }
}

/** A session on a fake canvas, with a clock the test moves by hand. */
export function startSession() {
  const surface = new FakeSurface()
  const metrics = new GridMetrics()
  const detach = metrics.attach(surface)
  const clock = { time: 1000 }
  const session = new PlaylistSession(surface, metrics, () => clock.time)
  return {
    surface,
    metrics,
    session,
    clock,
    stop() {
      session.destroy()
      detach()
    },
  }
}

export const project = () => useProjectStore.getState().project
export const tracks = () => project().playlist.tracks
export const history = () => useProjectStore.getState().history
/** The history as it is read: a label is kept as a key and made into words when shown. */
export const labels = () => history().entries.map((entry) => historyLabelText(entry.label))
export const ui = () => usePlaylistStore.getState()
export const selection = () => [...ui().selection].sort((a, b) => a - b)

/** The clips in the store, by row instead of by track id. */
export function clips() {
  const rows = new Map(tracks().map((track, row) => [track.id, row]))
  return project().playlist.clips.map((clip) => ({
    id: clip.id,
    row: rows.get(clip.track) ?? -1,
    start: clip.start,
    length: clip.length,
    offset: clip.offset,
    muted: clip.muted,
    pattern: clip.content.type === "pattern" ? clip.content.pattern : null,
  }))
}

/** The clips as "row:start+length" text, which reads well in an assertion. */
export const layout = () =>
  clips().map((clip) => `${clip.row}:${clip.start}+${clip.length}`)

type Modifiers = Partial<Pick<PointerInput, "shift" | "mod" | "alt" | "button">>

/** A pointer position at a tick and a row, in the middle of the row. */
export function at(
  tick: number,
  row: number,
  modifiers: Modifiers = {}
): PointerInput {
  return {
    x: tick * PX_PER_TICK,
    y: row * ROW_HEIGHT + ROW_HEIGHT / 2,
    button: 0,
    shift: false,
    mod: false,
    alt: false,
    ...modifiers,
  }
}

/** Presses at one place and releases at another, moving in a few steps. */
export async function drag(
  session: PlaylistSession,
  from: PointerInput,
  to: PointerInput
) {
  session.pointerDown(from)
  const steps = 4
  for (let step = 1; step <= steps; step++) {
    session.pointerMove({
      ...to,
      x: from.x + ((to.x - from.x) * step) / steps,
      y: from.y + ((to.y - from.y) * step) / steps,
    })
  }
  await session.pointerUp(to)
  await settle()
}

export async function click(session: PlaylistSession, point: PointerInput) {
  session.pointerDown(point)
  await session.pointerUp(point)
  await settle()
}

/** Types into the rename dialog, which the tests do not render. */
export async function answerText(text: string | null) {
  await settle()
  usePromptStore.getState().text?.resolve(text)
  await settle()
}

export async function answerConfirm(choice: string | null) {
  await settle()
  usePromptStore.getState().confirm?.resolve(choice)
  await settle()
}
