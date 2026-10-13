import type {
  ChannelId,
  Command,
  DispatchResult,
  Note,
  NoteArticulation,
  NoteExpression,
  NoteInit,
  NoteMusicalGrid,
  NoteTransform,
  NoteUpdate,
} from "@/bindings"
import {
  hitTestPoint,
  clampViewport,
  DEFAULT_LIMITS,
  keyToRow,
  queryRect,
  rowToKey,
  withAlpha,
  xToTick,
  yToRow,
  type Hit,
  type IndexedBatch,
  type Layer,
  type Marquee,
  type Rgba,
  type Viewport,
  type ViewportLimits,
} from "@/lib/canvas"
import { ignoresSnap } from "@/lib/edit-modifiers"
import { DEFAULT_NOTE_EXPRESSION } from "@/lib/note-expression"
import { useProjectStore } from "@/lib/store/project"
import { MAX_PATTERN_STEPS } from "@/lib/units"
import { meterSegments } from "@/lib/timeline"

import { readClipboard, writeClipboard } from "./clipboard"
import {
  cellsBetween,
  clampKey,
  copyNotes,
  curveInserts,
  noteInsertionCommand,
  noteEnd,
  duplicateRight,
  endAfterUpdates,
  endOfInits,
  initsFit,
  MAX_KEY,
  MAX_PATTERN_TICKS,
  moveDelta,
  moveLimits,
  moveUpdates,
  movedCopies,
  paintSpacing,
  pasteInits,
  pasteStart,
  resizeDelta,
  resizeUpdates,
  rowsAlongSegment,
  updatesFit,
  withExtension,
  type MoveDelta,
  type MoveLimits,
  type PasteTarget,
  type PatternInfo,
  type ResizeDelta,
  type ResizeEdge,
} from "./edit-math"
import { pressIntent, type Intent, type PressButton } from "./intents"
import {
  buildScene,
  FALLBACK_PALETTE,
  selectedNotes,
  type Scene,
} from "./scene"
import { snapFloor, snapRound } from "./snap"
import { mutedNoteIds } from "./select-muted"
import { nearestScaleKey, scaleMoveKeys, type PitchScale } from "./scales"
import { placeStamp, type Stamp, type StampPlacement } from "./stamps"
import type { Tool } from "./store"
import { sliceNoteCommand, toggledVelocity } from "./pointer-tools"
import { createDrumStroke, visitDrumCell, type DrumStroke } from "./drum-paint"

/** A pointer position in CSS pixels inside the grid, with the modifiers held. */
export type PointerInput = {
  x: number
  y: number
  shift: boolean
  ctrl: boolean
  alt: boolean
}

/** The part of the canvas view the editor drives. `TimeGridView` is one. */
export type EditorSurface = {
  readonly viewport: Viewport
  readonly limits?: ViewportLimits
  setViewport?(viewport: Viewport): void
  setItems(items: IndexedBatch | null): void
  setDragOffset(ticks: number, rows: number): void
  setDragResize(startTicks: number, endTicks: number, minLength?: number): void
  setMarquee(marquee: Marquee | null): void
  invalidate(layer?: Layer): void
}

export type EditorContext = {
  pattern: PatternInfo
  channel: ChannelId
  /** The lane's notes, sorted by start. The same array until they change. */
  notes: readonly Note[]
}

export type EditorSettings = {
  tool: Tool
  drum?: boolean
  /** Snap interval in ticks. 0 is off. */
  snap: number
  snapOrigin?: number
  snapEnd?: number
  musicalGrid?: NoteMusicalGrid | null
  lastLength: number
  lastVelocity: number
  articulation?: NoteArticulation
  glideTicks?: number
  colorGroup?: number | null
  /** Opt-in pitch policy, independent of the time grid. */
  pitchScale?: PitchScale | null
}

/** Everything the editor needs from outside, so tests can stand in for it. */
export type EditorHost = {
  generation?(): number
  dispatch(command: Command): Promise<DispatchResult | null>
  /** What is being edited, read from the project right now. */
  context(): EditorContext | null
  settings(tick?: number): EditorSettings
  remember(length: number, velocity: number): void
  /** Tells the user an edit was not made. */
  refuse(what: string, why: string): void
  noteOn(channel: ChannelId, key: number, velocity: number): void
  noteOff(channel: ChannelId, key: number): void
  seek?(tick: number): void
}

/** How a drag in progress shows the selected notes, before it is committed. */
export type DragPreview = {
  ticks: number
  keys: number
  resize: ResizeDelta | null
  /** The drop will copy the notes and leave the originals. */
  duplicate: boolean
}

export type Hover = {
  tick: number
  key: number
  intent: Intent | null
}

/**
 * - "scene": the notes on screen changed.
 * - "selection": which notes are selected changed.
 * - "drag": the preview of a drag in progress changed.
 * - "hover": what is under the pointer changed.
 * - "stamp": the armed pattern or its validation message changed.
 */
export type EditorEvent = "scene" | "selection" | "drag" | "hover" | "stamp"

export type StampState = { stamp: Stamp; error: string | null }

/** A menu choice owns its delayed completion only until the editor cancels it. */
export type PendingStampChoice = { complete(): boolean; cancel(): void }

type PendingStamp = {
  context: EditorContext
  generation: number | undefined
  tool: Tool
}

function sameStampContext(a: EditorContext, b: EditorContext | null): boolean {
  return (
    b !== null &&
    a.channel === b.channel &&
    a.pattern.id === b.pattern.id &&
    a.notes === b.notes &&
    a.pattern.lengthSteps === b.pattern.lengthSteps &&
    a.pattern.signature.numerator === b.pattern.signature.numerator &&
    a.pattern.signature.denominator === b.pattern.signature.denominator &&
    a.pattern.timeline === b.pattern.timeline
  )
}

type Press = { x: number; y: number }

type Gesture =
  | { kind: "idle" }
  | { kind: "stamp" }
  | {
      kind: "marquee"
      press: Press
      tick: number
      row: number
      base: ReadonlySet<number>
      hitId: number | null
      shift: boolean
      moved: boolean
    }
  | {
      kind: "move"
      press: Press
      tick: number
      row: number
      grabbed: Note
      notes: Note[]
      limits: MoveLimits
      delta: MoveDelta
      wasSelected: boolean
      shift: boolean
      /** The note is being drawn and is not in the project yet. */
      provisional: boolean
      moved: boolean
    }
  | {
      kind: "resize"
      press: Press
      tick: number
      edge: ResizeEdge
      grabbed: Note
      notes: Note[]
      delta: ResizeDelta
      moved: boolean
    }
  | {
      kind: "paint"
      origin: number
      spacing: number
      length: number
      velocity: number
      expression?: NoteExpression
      cells: Map<number, Note>
      lastCell: number
      lastTick: number
      /** The stroke reached past the longest pattern, where nothing is painted. */
      pastEnd: boolean
    }
  | { kind: "erase"; ids: Set<number>; tick: number; row: number }
  | {
      kind: "drum"
      stroke: DrumStroke
      tick: number
      row: number
      fallbackLength: number
      velocity: number
      expression?: NoteExpression
      pastEnd: boolean
    }
  | { kind: "mute"; note: Note | null }
  | { kind: "slice"; note: Note | null; tick: number }
  | { kind: "zoom"; press: Press; tick: number; row: number; tick1: number; row1: number; moved: boolean }
  | { kind: "playback" }

const IDLE: Gesture = { kind: "idle" }
const PAST_END = `Notes cannot go past step ${MAX_PATTERN_STEPS.toLocaleString("en-US")}`
// Kept to one line of a toast, which sits over the end of the tab row.
const PAST_END_WHY = "No pattern is longer, so it could never play."
/** A press has to travel this far before it counts as a drag. */
const DEAD_ZONE_PX = 3
/** Ids of notes that are drawn but not in the project yet count down from here. */
const PROVISIONAL_ID = -1000
const ERASED_ALPHA = 0.22

const noSnap = ignoresSnap

function drawnExpression(settings: EditorSettings): NoteExpression | undefined {
  if ((!settings.articulation || settings.articulation === "normal") && settings.colorGroup == null) return undefined
  return {
    ...DEFAULT_NOTE_EXPRESSION,
    articulation: settings.articulation ?? "normal",
    glideTicks: settings.glideTicks ?? 240,
    colorGroup: settings.colorGroup ?? undefined,
  }
}

/**
 * The piano roll without its canvas: the notes on screen, the selection,
 * the pointer gestures of each tool and the edits they turn into. Every
 * drag shows a preview and dispatches one command when it ends.
 */
export class Editor {
  hover: Hover | null = null

  private readonly host: EditorHost
  private surface: EditorSurface | null = null
  private ctx: EditorContext | null = null
  private palette: readonly Rgba[] = FALLBACK_PALETTE
  private scene: Scene = buildScene([], [], FALLBACK_PALETTE, new Set())
  private selected = new Set<number>()
  private provisional: Note[] = []
  private gesture: Gesture = IDLE
  private preview: DragPreview | null = null
  private sounding: number | null = null
  private listeners = new Set<(event: EditorEvent) => void>()
  private armedStamp: StampState | null = null
  private pendingStamp: PendingStamp | null = null
  private stampLane: EditorContext | null = null
  private stampGeneration: number | undefined
  private disposed = false
  private previousZoom: Viewport | null = null

  constructor(host: EditorHost) {
    this.host = host
  }

  subscribe(listener: (event: EditorEvent) => void): () => void {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  attach(surface: EditorSurface | null): void {
    // React Strict Mode cleans up then reattaches the same session on mount.
    if (surface) this.disposed = false
    this.surface = surface
    surface?.setItems(this.scene.items)
  }

  get context(): EditorContext | null {
    return this.ctx
  }

  get notes(): readonly Note[] {
    return this.scene.notes
  }

  get items(): IndexedBatch {
    return this.scene.items
  }

  get selection(): ReadonlySet<number> {
    return this.selected
  }

  get drag(): DragPreview | null {
    return this.preview
  }

  /** How many notes of the project are selected. */
  get selectionCount(): number {
    let count = 0
    for (const id of this.selected) if (id >= 0) count++
    return count
  }

  get busy(): boolean {
    return this.hasPointerGesture || this.hasStampChoice
  }

  get canRestoreZoom(): boolean {
    return this.previousZoom !== null
  }

  get scrubbing(): boolean {
    return this.gesture.kind === "playback"
  }

  /** Only a started pointer gesture owns capture, not an idle stamp choice. */
  get hasPointerGesture(): boolean {
    return this.gesture.kind !== "idle"
  }

  get hasStampChoice(): boolean {
    return this.armedStamp !== null || this.pendingStamp !== null
  }

  get stampState(): StampState | null {
    return this.armedStamp
  }

  /** Reserve a menu choice while its exit transition still owns keyboard focus. */
  deferStamp(stamp: Stamp): PendingStampChoice | null {
    const context = this.ctx
    if (!context || this.disposed) return null
    this.cancel()
    const choice: PendingStamp = {
      context,
      generation: this.host.generation?.(),
      tool: this.host.settings().tool,
    }
    this.pendingStamp = choice
    this.emit("stamp")
    return {
      complete: () => {
        if (this.pendingStamp !== choice) return false
        this.cancel()
        if (
          this.disposed ||
          choice.generation !== this.host.generation?.() ||
          choice.tool !== this.host.settings().tool ||
          !sameStampContext(context, this.ctx) ||
          !sameStampContext(context, this.host.context())
        )
          return false
        this.armStamp(stamp)
        return true
      },
      cancel: () => {
        if (this.pendingStamp === choice) this.cancel()
      },
    }
  }

  /** Arm a one-shot placement. No project edit happens until the click ends. */
  armStamp(stamp: Stamp): void {
    if (!this.ctx || this.disposed) return
    this.cancel()
    this.armedStamp = { stamp, error: null }
    this.stampLane = this.ctx
    this.stampGeneration = this.host.generation?.()
    this.emit("stamp")
  }

  selectedNotes(): Note[] {
    return selectedNotes(this.scene, this.selected)
  }

  /** Colors change with the theme and with the channel. */
  setPalette(palette: readonly Rgba[]): void {
    this.palette = palette.length > 0 ? palette : FALLBACK_PALETTE
    this.rebuild()
  }

  /**
   * Follows the project. Called on every render of the panel and after
   * every commit; it only does work when the lane really changed.
   */
  setContext(next: EditorContext | null): void {
    if (this.pendingStamp && !sameStampContext(this.pendingStamp.context, next))
      this.cancel()
    this.follow(next, false)
  }

  private follow(next: EditorContext | null, force: boolean): void {
    const previous = this.ctx
    this.ctx = next
    if (!next) {
      this.abandon()
      this.selected = new Set()
      this.rebuild()
      this.emit("selection")
      return
    }
    const sameLane =
      previous?.channel === next.channel &&
      previous.pattern.id === next.pattern.id
    if (
      this.armedStamp &&
      (!sameLane ||
        previous?.pattern.lengthSteps !== next.pattern.lengthSteps ||
        previous.pattern.signature.numerator !==
          next.pattern.signature.numerator ||
        previous.pattern.signature.denominator !==
          next.pattern.signature.denominator ||
        previous.pattern.timeline !== next.pattern.timeline)
    ) {
      this.cancel()
    }
    if (sameLane && previous.notes === next.notes && !force) return
    this.abandon()
    if (!sameLane) {
      this.selected = new Set()
    } else if (this.selected.size > 0) {
      // Undo or another window can remove notes that were selected.
      const alive = new Set<number>()
      for (const note of next.notes) {
        if (this.selected.has(note.id)) alive.add(note.id)
      }
      this.selected = alive
    }
    this.rebuild()
    this.emit("selection")
  }

  // Pointer gestures

  /** The intent a press at this point would have, for the cursor and hint. */
  pointerHover(input: PointerInput): void {
    const surface = this.surface
    if (!surface || this.gesture.kind !== "idle") return
    if (this.armedStamp) {
      this.previewStamp(input)
      return
    }
    const hit = this.hitAt(input)
    const { tool, drum } = this.host.settings()
    this.setHover(
      xToTick(surface.viewport, input.x),
      this.keyAt(input.y),
      pressIntent(tool, "left", hit?.part ?? null, input, drum)
    )
  }

  pointerLeave(): void {
    if (this.gesture.kind !== "idle" || this.hover === null) return
    this.hover = null
    if (this.armedStamp) {
      this.provisional = []
      this.rebuild()
    }
    this.emit("hover")
  }

  /** Starts a gesture. Returns what the press does. */
  pointerDown(input: PointerInput, button: PressButton): Intent | null {
    if (button === "right" && this.hasStampChoice) {
      this.cancel()
      return null
    }
    const surface = this.surface
    const ctx = this.ctx
    if (!surface || !ctx) return null
    if (this.armedStamp) {
      const placement = this.previewStamp(input)
      if (!placement) return null
      if (placement.error) {
        this.host.refuse("Stamp was not placed", placement.error)
        return null
      }
      this.gesture = { kind: "stamp" }
      return { kind: "draw" }
    }
    if (this.gesture.kind !== "idle") this.cancel()
    const tick = xToTick(surface.viewport, input.x)
    const settings = this.host.settings(tick)
    const hit = this.hitAt(input)
    const intent = pressIntent(settings.tool, button, hit?.part ?? null, input, settings.drum)
    const row = yToRow(surface.viewport, input.y)
    const press = { x: input.x, y: input.y }

    switch (intent.kind) {
      case "mute":
        this.gesture = { kind: "mute", note: hit ? this.scene.notes[hit.index] : null }
        break
      case "slice":
        this.gesture = { kind: "slice", note: hit ? this.scene.notes[hit.index] : null,
          tick: snapRound(tick, noSnap(input) ? 0 : settings.snap, settings.snapOrigin) }
        break
      case "zoom":
        if (this.canRestoreZoom) this.restoreZoom()
        else this.gesture = { kind: "zoom", press, tick, row, tick1: tick, row1: row, moved: false }
        break
      case "playback":
        this.gesture = { kind: "playback" }
        this.scrub(tick, input)
        break
      case "menu":
        if (hit && !this.selected.has(hit.id)) this.setSelection([hit.id])
        break
      case "marquee":
        this.gesture = {
          kind: "marquee",
          press,
          tick,
          row,
          base: input.shift ? new Set(this.selected) : new Set(),
          hitId: hit?.id ?? null,
          shift: input.shift,
          moved: false,
        }
        break
      case "erase":
        this.gesture = { kind: "erase", ids: new Set(), tick, row }
        if (hit) this.markErased([hit.index])
        this.eraseAlong(tick, row, tick, row)
        break
      case "move":
        if (hit) this.beginMove(hit, input, press, tick, row)
        break
      case "resize":
        if (hit) this.beginResize(hit, intent.edge, input, press, tick)
        break
      case "draw":
        this.beginDraw(input, press, tick, row, settings)
        break
      case "paint":
        if (settings.drum) {
          this.gesture = {
            kind: "drum",
            stroke: createDrumStroke(),
            tick,
            row,
            fallbackLength: settings.lastLength,
            velocity: settings.lastVelocity,
            expression: drawnExpression(settings),
            pastEnd: false,
          }
          this.drumAlong(this.gesture, tick, row)
        } else this.beginPaint(input, tick, row, settings)
        break
      default: {
        const _exhaustive: never = intent
        return _exhaustive
      }
    }
    return intent
  }

  pointerMove(input: PointerInput): void {
    const surface = this.surface
    const gesture = this.gesture
    if (!surface) return
    const viewport = surface.viewport
    const tick = xToTick(viewport, input.x)
    const row = yToRow(viewport, input.y)

    switch (gesture.kind) {
      case "mute":
      case "slice":
        return
      case "zoom": {
        if (!gesture.moved && !pastDeadZone(gesture.press, input)) return
        gesture.moved = true
        gesture.tick1 = xToTick(viewport, Math.max(0, Math.min(viewport.width, input.x)))
        gesture.row1 = yToRow(viewport, Math.max(0, Math.min(viewport.height, input.y)))
        surface.setMarquee({ tick0: gesture.tick, row0: gesture.row, tick1: gesture.tick1, row1: gesture.row1 })
        return
      }
      case "playback":
        this.scrub(tick, input)
        return
      case "stamp":
        this.previewStamp(input)
        return
      case "idle":
        this.pointerHover(input)
        return
      case "marquee": {
        if (!gesture.moved && !pastDeadZone(gesture.press, input)) return
        gesture.moved = true
        surface.setMarquee({
          tick0: gesture.tick,
          row0: gesture.row,
          tick1: tick,
          row1: row,
        })
        const batch = this.scene.items.batch
        const indices = queryRect(
          this.scene.items,
          Math.min(gesture.tick, tick),
          Math.max(gesture.tick, tick),
          Math.min(gesture.row, row),
          Math.max(gesture.row, row)
        )
        const ids = new Set(gesture.base)
        for (const index of indices) ids.add(batch.ids[index])
        this.applySelection(ids)
        this.setHover(tick, this.keyAt(input.y), { kind: "marquee" })
        return
      }
      case "move": {
        if (!gesture.moved && !pastDeadZone(gesture.press, input)) return
        gesture.moved = true
        const anchor = gesture.grabbed.start + tick - gesture.tick
        const settings = this.host.settings(anchor)
        const snap = noSnap(input) ? 0 : settings.snap
        const local = !!this.ctx?.pattern.timeline?.meters.length
        const delta = moveDelta(
          local ? snapRound(anchor, snap, settings.snapOrigin) - gesture.grabbed.start : tick - gesture.tick,
          Math.floor(gesture.row) - Math.floor(row),
          local ? 0 : snap,
          gesture.limits
        )
        const rawKeys = gesture.provisional
          ? rowToKey(Math.floor(row)) - gesture.grabbed.key
          : Math.floor(gesture.row) - Math.floor(row)
        delta.keys = scaleMoveKeys(
          gesture.grabbed.key,
          rawKeys,
          gesture.limits,
          noSnap(input) ? null : (this.host.settings().pitchScale ?? null)
        )
        gesture.delta = delta
        surface.setDragOffset(delta.ticks, -delta.keys)
        this.preview = {
          ticks: delta.ticks,
          keys: delta.keys,
          resize: null,
          duplicate: input.ctrl && !gesture.provisional,
        }
        const key = gesture.grabbed.key + delta.keys
        this.sound(key, gesture.grabbed.velocity)
        this.setHover(gesture.grabbed.start + delta.ticks, key, {
          kind: gesture.provisional ? "draw" : "move",
        })
        this.emit("drag")
        return
      }
      case "resize": {
        if (!gesture.moved && !pastDeadZone(gesture.press, input)) return
        gesture.moved = true
        const anchor = gesture.edge === "start" ? gesture.grabbed.start : noteEnd(gesture.grabbed)
        const settings = this.host.settings(anchor + tick - gesture.tick)
        const snap = noSnap(input) ? 0 : settings.snap
        const delta = resizeDelta(
          gesture.edge,
          tick - gesture.tick,
          snap,
          gesture.notes,
          this.ctx?.pattern.timeline?.meters.length ? { anchor, origin: settings.snapOrigin ?? 0 } : undefined
        )
        gesture.delta = delta
        surface.setDragResize(delta.start, delta.end, delta.minLength)
        this.preview = { ticks: 0, keys: 0, resize: delta, duplicate: false }
        this.emit("drag")
        return
      }
      case "drum":
        this.drumAlong(gesture, tick, row)
        this.setHover(tick, this.keyAt(input.y), { kind: "paint", drum: true })
        return
      case "paint": {
        if (this.ctx?.pattern.timeline?.meters.length && !noSnap(input)) {
          this.paintCells(gesture, this.meterPaintStarts(gesture, gesture.lastTick, tick), row, input)
          gesture.lastTick = tick
          this.setHover(tick, this.keyAt(input.y), { kind: "paint" })
          return
        }
        const cell = Math.floor((tick - gesture.origin) / gesture.spacing)
        const cells = cellsBetween(gesture.lastCell, cell)
        gesture.lastCell = cell
        gesture.lastTick = tick
        this.paintCells(gesture, cells.map((cell) => gesture.origin + cell * gesture.spacing), row, input)
        this.setHover(tick, this.keyAt(input.y), { kind: "paint" })
        return
      }
      case "erase":
        this.eraseAlong(gesture.tick, gesture.row, tick, row)
        gesture.tick = tick
        gesture.row = row
        this.setHover(tick, this.keyAt(input.y), { kind: "erase" })
        return
      default: {
        const _exhaustive: never = gesture
        return _exhaustive
      }
    }
  }

  /** Ends the gesture and commits it as one command. */
  pointerUp(input: PointerInput): void {
    const gesture = this.gesture
    const ctx = this.ctx
    if (gesture.kind === "idle") return
    this.pointerMove(input)
    this.gesture = IDLE
    this.sound(null)
    this.surface?.setMarquee(null)
    if (!ctx) {
      this.abandon()
      return
    }

    switch (gesture.kind) {
      case "mute":
        if (gesture.note) this.commitDrag({ type: "updateNotes", pattern: ctx.pattern.id, channel: ctx.channel,
          updates: [{ id: gesture.note.id, patch: { velocity: toggledVelocity(ctx.pattern.id, ctx.channel, gesture.note, this.host.settings().lastVelocity) } }] })
        break
      case "slice": {
        if (!gesture.note) break
        const command = sliceNoteCommand({ pattern: ctx.pattern.id, channel: ctx.channel }, gesture.note, gesture.tick, ctx.pattern.noteCurves)
        if (command) this.commitDrag(command)
        break
      }
      case "zoom": {
        const surface = this.surface
        const ticks = Math.abs(gesture.tick1 - gesture.tick)
        const rows = Math.abs(gesture.row1 - gesture.row)
        if (!surface?.setViewport || !gesture.moved || ticks <= 0 || rows <= 0) break
        this.previousZoom = { ...surface.viewport }
        surface.setViewport(clampViewport({ ...surface.viewport,
          scrollTick: Math.min(gesture.tick, gesture.tick1), scrollRow: Math.min(gesture.row, gesture.row1),
          pxPerTick: surface.viewport.width / ticks, rowHeight: surface.viewport.height / rows,
        }, surface.limits ?? DEFAULT_LIMITS))
        this.emit("drag")
        break
      }
      case "playback":
        break
      case "stamp": {
        const stamp = this.armedStamp
        if (!stamp || stamp.error) break
        const notes: NoteInit[] = this.provisional.map(
          ({ start, length, key, velocity, pan, expression }) => ({
            start,
            length,
            key,
            velocity,
            pan,
            expression: expression ? { ...expression } : undefined,
          })
        )
        this.cancel()
        if (notes.length === 0) break
        this.commitDrag(
          withExtension(
            {
              type: "addNotes",
              pattern: ctx.pattern.id,
              channel: ctx.channel,
              notes,
            },
            `Stamp ${stamp.stamp.label}`,
            ctx.pattern,
            endOfInits(notes)
          ),
          true
        )
        break
      }
      case "marquee":
        if (gesture.moved) break
        if (gesture.hitId === null) {
          if (!gesture.shift) this.setSelection([])
        } else if (gesture.shift) {
          this.toggleSelected(gesture.hitId)
        } else {
          this.setSelection([gesture.hitId])
        }
        break
      case "move":
        this.finishMove(gesture, input, ctx)
        break
      case "resize": {
        const updates = resizeUpdates(gesture.notes, gesture.delta)
        if (updates.length === 0) {
          this.clearPreview()
          break
        }
        if (!updatesFit(gesture.notes, updates)) {
          this.refusePastEnd()
          break
        }
        const grabbed = updates.find(
          (update) => update.id === gesture.grabbed.id
        )
        this.host.remember(
          grabbed?.patch.length ?? gesture.grabbed.length,
          gesture.grabbed.velocity
        )
        this.commitDrag(
          withExtension(
            {
              type: "updateNotes",
              pattern: ctx.pattern.id,
              channel: ctx.channel,
              updates,
            },
            "Resize notes",
            ctx.pattern,
            endAfterUpdates(gesture.notes, updates)
          )
        )
        break
      }
      case "drum": {
        const { adds, deletes } = gesture.stroke
        if (gesture.pastEnd) this.host.refuse(PAST_END, PAST_END_WHY)
        const commands: Command[] = []
        if (deletes.size) commands.push({
          type: "removeNotes",
          pattern: ctx.pattern.id,
          channel: ctx.channel,
          notes: [...deletes],
        })
        if (adds.length) commands.push({
          type: "addNotes",
          pattern: ctx.pattern.id,
          channel: ctx.channel,
          notes: adds,
        })
        if (commands.length) this.commitDrag(withExtension(
          { type: "batch", label: "Paint drum steps", commands },
          "Paint drum steps",
          ctx.pattern,
          endOfInits(adds)
        ))
        break
      }
      case "paint": {
        const painted = [...gesture.cells.values()].sort(
          (a, b) => a.start - b.start
        )
        // What fits is kept; the rest of the stroke is explained.
        if (gesture.pastEnd) this.host.refuse(PAST_END, PAST_END_WHY)
        if (painted.length === 0) break
        const notes = movedCopies(painted, { ticks: 0, keys: 0 })
        this.commitDrag(
          withExtension(
            {
              type: "addNotes",
              pattern: ctx.pattern.id,
              channel: ctx.channel,
              notes,
            },
            "Paint notes",
            ctx.pattern,
            endOfInits(notes)
          )
        )
        break
      }
      case "erase": {
        if (gesture.ids.size === 0) break
        const notes = [...gesture.ids]
        for (const id of notes) this.selected.delete(id)
        this.commitDrag({
          type: "removeNotes",
          pattern: ctx.pattern.id,
          channel: ctx.channel,
          notes,
        })
        break
      }
      default: {
        const _exhaustive: never = gesture
        return _exhaustive
      }
    }
    this.pointerHover(input)
  }

  /** Drops the gesture in progress and puts everything back. */
  cancel(): void {
    this.restoreZoom()
    const gesture = this.gesture
    if (gesture.kind === "idle" && !this.armedStamp && !this.pendingStamp)
      return
    if (gesture.kind === "marquee") this.selected = new Set(gesture.base)
    this.abandon()
    this.rebuild()
    this.emit("selection")
  }

  /** Stops any note that is sounding and lets go of the canvas. */
  dispose(): void {
    this.disposed = true
    this.abandon()
    this.surface = null
  }

  // Selection

  setSelection(ids: Iterable<number>): void {
    this.applySelection(new Set(ids))
  }

  selectAll(): void {
    this.setSelection(this.scene.notes.map((note) => note.id))
  }

  /** Expands the selection to every note on the selected pitches. */
  selectMatchingPitches(): void {
    const keys = new Set(this.selectedNotes().map((note) => note.key))
    if (keys.size === 0) return
    this.setSelection(
      this.scene.notes.filter((note) => keys.has(note.key)).map((note) => note.id)
    )
  }

  /** Replaces the selection with every muted note in the open channel. */
  selectMutedNotes(): void {
    this.setSelection(mutedNoteIds(this.scene.notes))
  }

  /** Selects every note on one key. With `add`, keeps what is selected. */
  selectKey(key: number, add: boolean): void {
    const ids = add ? new Set(this.selected) : new Set<number>()
    for (const note of this.scene.notes) {
      if (note.key === key) ids.add(note.id)
    }
    this.applySelection(ids)
  }

  // Edits from the keyboard and menus. Each is one command.

  async deleteSelection(): Promise<void> {
    const ctx = this.ctx
    if (!ctx || this.selected.size === 0) return
    const notes = [...this.selected]
    this.selected = new Set()
    await this.commit({
      type: "removeNotes",
      pattern: ctx.pattern.id,
      channel: ctx.channel,
      notes,
    })
  }

  copy(): boolean {
    const contents = copyNotes(this.selectedNotes(), this.ctx?.pattern.noteCurves)
    if (!contents) return false
    writeClipboard(contents)
    return true
  }

  async cut(): Promise<void> {
    if (this.copy()) await this.deleteSelection()
  }

  /** Pastes the clipboard and selects what was pasted. */
  async paste(target: PasteTarget): Promise<void> {
    const ctx = this.ctx
    const contents = readClipboard()
    if (!ctx || !contents) return
    const start = pasteStart(
      contents,
      target,
      this.host.settings().snap,
      ctx.pattern.signature,
      ctx.pattern.timeline,
      (tick) => this.host.settings(tick)
    )
    let notes = pasteInits(contents, start)
    const scale = this.host.settings().pitchScale
    if (scale && notes.length > 0) {
      const anchor = notes.reduce((key, note) => Math.min(key, note.key), 127)
      const high = notes.reduce((key, note) => Math.max(key, note.key), 0)
      const snapped = nearestScaleKey(anchor, scale, 0, 127 - high + anchor)
      if (snapped === null) {
        this.host.refuse(
          "Notes were not pasted",
          "No scale root fits without changing the group's intervals."
        )
        return
      }
      notes = notes.map((note) => ({
        ...note,
        key: note.key + snapped - anchor,
      }))
    }
    if (!initsFit(notes)) return this.refusePastEnd()
    await this.commit(
      withExtension(
        noteInsertionCommand({ pattern: ctx.pattern.id, channel: ctx.channel }, notes, contents.curves),
        "Paste notes",
        ctx.pattern,
        endOfInits(notes)
      ),
      true
    )
  }

  /** Copies the selection to right after itself and selects the copy. */
  async duplicate(): Promise<void> {
    const ctx = this.ctx
    if (!ctx) return
    const sources = this.selectedNotes()
    const { inits } = duplicateRight(
      sources,
      this.host.settings().snap
    )
    if (inits.length === 0) return
    if (!initsFit(inits)) return this.refusePastEnd()
    await this.commit(
      withExtension(
        noteInsertionCommand({ pattern: ctx.pattern.id, channel: ctx.channel }, inits, curveInserts(sources, ctx.pattern.noteCurves)),
        "Duplicate notes",
        ctx.pattern,
        endOfInits(inits)
      ),
      true
    )
  }

  get snapInterval(): number {
    return this.host.settings(this.selectedNotes()[0]?.start ?? 0).snap
  }

  get snapMusicalGrid(): NoteMusicalGrid | null {
    return this.host.settings(this.selectedNotes()[0]?.start ?? 0).musicalGrid ?? null
  }

  /** Shared Rust tools validate captured notes and commit once, without previews. */
  async transformNotes(
    notes: readonly Note[],
    transform: NoteTransform
  ): Promise<boolean> {
    const ctx = this.ctx
    if (!ctx || this.busy || notes.length === 0) return false
    const result = await this.host.dispatch({
      type: "transformNotes",
      pattern: ctx.pattern.id,
      channel: ctx.channel,
      notes: [...notes],
      transform,
    })
    // Keep surviving selected ids and include newly chopped segments.
    if (result) this.selected = new Set([...this.selected, ...result.created])
    this.follow(this.host.context(), true)
    return result !== null
  }

  /** Quick straight quantize, selection only; the action opens the options dialog. */
  async quantize(edge: ResizeEdge): Promise<void> {
    const { snap, musicalGrid } = this.host.settings(this.selectedNotes()[0]?.start ?? 0)
    if (snap <= 0) return
    await this.transformNotes(this.selectedNotes(), {
      type: "quantize",
      grid: snap,
      strength: 1,
      edge,
      groove: "straight",
      musical: musicalGrid ?? undefined,
    })
  }

  /** Moves the selection by snap intervals and semitones. */
  async nudge(steps: number, keys: number): Promise<void> {
    const notes = this.selectedNotes()
    const anchor = notes.reduce((earliest, note) => Math.min(earliest, note.start), Infinity)
    const { snap } = this.host.settings(Number.isFinite(anchor) ? anchor : 0)
    let target = anchor
    for (let remaining = Math.min(1024, Math.abs(steps)); Number.isFinite(target) && remaining > 0; remaining--) {
      const settings = this.host.settings(steps < 0 ? Math.max(0, target - 1) : target)
      const spacing = settings.snap > 0 ? settings.snap : 1
      const origin = settings.snapOrigin ?? 0
      const candidate = steps < 0 ? origin + (Math.ceil((target - origin) / spacing) - 1) * spacing : origin + (Math.floor((target - origin) / spacing) + 1) * spacing
      target = Math.max(0, steps < 0 ? Math.max(origin, candidate) : Math.min(settings.snapEnd ?? Infinity, candidate))
    }
    // With snap off a nudge is the finest move there is: one tick.
    const delta = moveDelta(
      this.ctx?.pattern.timeline?.meters.length && notes.length > 0 ? target - anchor : steps * (snap > 0 ? snap : 1),
      keys,
      0,
      moveLimits(notes)
    )
    if (notes.length > 0)
      delta.keys = scaleMoveKeys(
        notes.reduce((key, note) => Math.min(key, note.key), 127),
        keys,
        moveLimits(notes),
        this.host.settings().pitchScale ?? null,
        true
      )
    await this.update(notes, moveUpdates(notes, delta), "Move notes")
  }

  /** Transposes the selection, preserving its shape within the keyboard. */
  async transpose(semitones: number): Promise<void> {
    const notes = this.selectedNotes()
    const delta = moveDelta(0, semitones, 0, moveLimits(notes))
    if (notes.length > 0)
      delta.keys = scaleMoveKeys(
        notes.reduce((key, note) => Math.min(key, note.key), 127),
        semitones,
        moveLimits(notes),
        this.host.settings().pitchScale ?? null,
        true
      )
    await this.update(notes, moveUpdates(notes, delta), "Transpose notes")
  }

  /** Dispatches note updates as one command and follows the result. */
  async update(
    notes: readonly Note[],
    updates: readonly NoteUpdate[],
    label: string
  ): Promise<void> {
    const ctx = this.ctx
    if (!ctx || updates.length === 0) return
    if (!updatesFit(notes, updates)) return this.refusePastEnd()
    await this.commit(
      withExtension(
        {
          type: "updateNotes",
          pattern: ctx.pattern.id,
          channel: ctx.channel,
          updates: [...updates],
        },
        label,
        ctx.pattern,
        endAfterUpdates(notes, updates)
      )
    )
  }

  // Internals

  private emit(event: EditorEvent): void {
    for (const listener of [...this.listeners]) listener(event)
  }

  private restoreZoom(): void {
    if (!this.previousZoom) return
    const previous = this.previousZoom
    this.previousZoom = null
    const surface = this.surface
    if (surface) surface.setViewport?.({ ...surface.viewport,
      scrollTick: previous.scrollTick, scrollRow: previous.scrollRow,
      pxPerTick: previous.pxPerTick, rowHeight: previous.rowHeight })
    this.emit("drag")
  }

  private scrub(tick: number, input: PointerInput): void {
    const last = (this.ctx?.pattern.lengthSteps ?? 16) * 240 - 1
    const position = Math.max(0, Math.min(last, Math.round(tick)))
    this.host.seek?.(position)
    const key = this.keyAt(input.y)
    const note = this.ctx?.notes.find((note) => note.key === key && note.start <= position && noteEnd(note) > position && note.velocity > 0 && note.expression?.articulation !== "slide")
    this.sound(note?.key ?? null, note?.velocity)
    this.setHover(position, key, { kind: "playback" })
  }

  private previewStamp(input: PointerInput): StampPlacement | null {
    const armed = this.armedStamp
    const lane = this.stampLane
    const live = this.host.context()
    const surface = this.surface
    if (!armed || !lane || !surface) return null
    if (
      this.stampGeneration !== this.host.generation?.() ||
      live?.channel !== lane.channel ||
      live?.pattern.id !== lane.pattern.id ||
      live?.notes !== lane.notes ||
      live?.pattern.lengthSteps !== lane.pattern.lengthSteps ||
      live?.pattern.signature.numerator !== lane.pattern.signature.numerator ||
      live?.pattern.signature.denominator !== lane.pattern.signature.denominator ||
      live?.pattern.timeline !== lane.pattern.timeline
    ) {
      this.cancel()
      return null
    }
    const settings = this.host.settings(xToTick(surface.viewport, input.x))
    const tick = Math.round(
      snapFloor(
        xToTick(surface.viewport, input.x),
        noSnap(input) ? 0 : settings.snap,
        settings.snapOrigin
      )
    )
    const rawKey = rowToKey(Math.floor(yToRow(surface.viewport, input.y)))
    const key =
      !noSnap(input) && settings.pitchScale && rawKey >= 0 && rawKey <= 127
        ? (nearestScaleKey(rawKey, settings.pitchScale) ?? rawKey)
        : rawKey
    const rawPlacement = placeStamp(
      armed.stamp,
      tick,
      key,
      settings.lastLength,
      settings.lastVelocity
    )
    const placement: StampPlacement = rawPlacement.error !== null ? rawPlacement : {
      ...rawPlacement,
      notes: rawPlacement.notes.map((note) => ({ ...note, expression: note.expression ?? drawnExpression(settings) })),
    }
    if (armed.error !== placement.error) {
      this.armedStamp = { ...armed, error: placement.error }
      this.emit("stamp")
    }
    const changed =
      placement.notes.length !== this.provisional.length ||
      placement.notes.some((note, index) => {
        const shown = this.provisional[index]
        return (
          note.start !== shown.start ||
          note.key !== shown.key ||
          note.length !== shown.length ||
          note.velocity !== shown.velocity ||
          (note.expression?.colorGroup ?? null) !== (shown.expression?.colorGroup ?? null) ||
          (note.expression?.articulation ?? "normal") !== (shown.expression?.articulation ?? "normal") ||
          (note.expression?.glideTicks ?? 240) !== (shown.expression?.glideTicks ?? 240)
        )
      })
    if (changed) {
      this.provisional = placement.notes.map((note, index) => ({
        ...note,
        id: PROVISIONAL_ID - index,
        velocity: note.velocity ?? settings.lastVelocity,
        pan: note.pan ?? 0,
      }))
      this.rebuild()
    }
    this.setHover(tick, key, { kind: "draw" })
    return placement
  }

  private hitAt(input: PointerInput): Hit | null {
    const surface = this.surface
    if (!surface) return null
    return hitTestPoint(surface.viewport, this.scene.items, input.x, input.y)
  }

  private keyAt(y: number): number {
    const surface = this.surface
    if (!surface) return 0
    return clampKey(rowToKey(Math.floor(yToRow(surface.viewport, y))))
  }

  private setHover(tick: number, key: number, intent: Intent | null): void {
    const next: Hover = { tick: Math.max(0, tick), key, intent }
    const previous = this.hover
    this.hover = next
    if (
      previous &&
      Math.round(previous.tick) === Math.round(next.tick) &&
      previous.key === next.key &&
      intentName(previous.intent) === intentName(next.intent)
    ) {
      return
    }
    this.emit("hover")
  }

  private rebuild(): void {
    const notes = this.ctx?.notes ?? []
    const deleted = this.gesture.kind === "drum" ? this.gesture.stroke.deletes : null
    this.scene = buildScene(
      deleted?.size ? notes.filter((note) => !deleted.has(note.id)) : notes,
      this.provisional,
      this.palette,
      this.armedStamp
        ? new Set([
            ...this.selected,
            ...this.provisional.map((note) => note.id),
          ])
        : this.selected
    )
    this.surface?.setItems(this.scene.items)
    this.emit("scene")
  }

  /** Changes the selection in place: flags only, no rebuild. */
  private applySelection(ids: Set<number>): void {
    const ctx = this.ctx
    if (ctx) {
      const live = new Set(ctx.notes.map((note) => note.id))
      for (const group of useProjectStore.getState().project.noteEditGroups ?? []) {
        if (group.pattern !== ctx.pattern.id || group.channel !== ctx.channel) continue
        if (group.notes.some((id) => ids.has(id))) {
          for (const id of group.notes) if (live.has(id)) ids.add(id)
        }
      }
    }
    this.selected = ids
    const batch = this.scene.items.batch
    const indices: number[] = []
    for (const id of ids) {
      const index = batch.indexOfId(id)
      if (index >= 0) indices.push(index)
    }
    batch.setSelection(indices)
    this.surface?.invalidate("base")
    this.emit("selection")
  }

  private toggleSelected(id: number): void {
    const ids = new Set(this.selected)
    if (ids.delete(id)) {
      const ctx = this.ctx
      for (const group of useProjectStore.getState().project.noteEditGroups ?? []) {
        if (ctx && group.pattern === ctx.pattern.id && group.channel === ctx.channel && group.notes.includes(id)) {
          for (const member of group.notes) ids.delete(member)
        }
      }
    } else ids.add(id)
    this.applySelection(ids)
  }

  private sound(key: number | null, velocity?: number): void {
    const ctx = this.ctx
    if (key === this.sounding) return
    if (this.sounding !== null && ctx) {
      this.host.noteOff(ctx.channel, this.sounding)
    }
    this.sounding = null
    if (key === null || !ctx || key < 0 || key > MAX_KEY) return
    this.sounding = key
    this.host.noteOn(
      ctx.channel,
      key,
      velocity ?? this.host.settings().lastVelocity
    )
  }

  private clearPreview(): void {
    this.surface?.setDragOffset(0, 0)
    this.surface?.setDragResize(0, 0)
    if (this.preview !== null) {
      this.preview = null
      this.emit("drag")
    }
  }

  /** Ends whatever is in progress without committing it. No rebuild. */
  private abandon(): void {
    this.restoreZoom()
    this.sound(null)
    this.gesture = IDLE
    if (this.armedStamp || this.pendingStamp) {
      this.armedStamp = null
      this.pendingStamp = null
      this.stampLane = null
      this.emit("stamp")
    }
    if (this.provisional.length > 0) {
      for (const note of this.provisional) this.selected.delete(note.id)
      this.provisional = []
    }
    this.surface?.setMarquee(null)
    this.clearPreview()
  }

  /**
   * Turns down an edit that would put a note past the longest pattern
   * there can be, where it could never play, and puts the preview back.
   */
  private refusePastEnd(): void {
    this.host.refuse(PAST_END, PAST_END_WHY)
    this.follow(this.host.context(), true)
  }

  /**
   * Dispatches a command and follows the project to its result. With
   * `selectCreated` the notes it made become the selection.
   */
  private async commit(command: Command, selectCreated = false): Promise<void> {
    const ctx = this.ctx
    const generation = this.host.generation?.()
    const result = await this.host.dispatch(command)
    if (
      this.disposed ||
      generation !== this.host.generation?.() ||
      ctx?.channel !== this.ctx?.channel ||
      ctx?.pattern.id !== this.ctx?.pattern.id
    )
      return
    if (result && selectCreated) this.selected = new Set(result.created)
    // Forced, because a command that failed leaves the lane as it was and
    // the preview still has to go.
    this.follow(this.host.context(), true)
  }

  /**
   * Commits the end of a drag. The preview stays up until the project
   * answers, so the notes never jump back for a frame.
   */
  private commitDrag(command: Command, selectCreated = false): void {
    void this.commit(command, selectCreated)
  }

  private beginMove(
    hit: Hit,
    input: PointerInput,
    press: Press,
    tick: number,
    row: number
  ): void {
    const grabbed = this.scene.notes[hit.index]
    const wasSelected = this.selected.has(grabbed.id)
    if (!wasSelected) {
      this.setSelection(
        input.shift ? [...this.selected, grabbed.id] : [grabbed.id]
      )
    }
    const notes = this.selectedNotes()
    this.gesture = {
      kind: "move",
      press,
      tick,
      row,
      grabbed,
      notes,
      limits: moveLimits(notes),
      delta: { ticks: 0, keys: 0 },
      wasSelected,
      shift: input.shift,
      provisional: false,
      moved: false,
    }
    this.host.remember(grabbed.length, grabbed.velocity)
    this.sound(grabbed.key, grabbed.velocity)
  }

  private finishMove(
    gesture: Extract<Gesture, { kind: "move" }>,
    input: PointerInput,
    ctx: EditorContext
  ): void {
    const { delta, notes } = gesture
    const target = { pattern: ctx.pattern.id, channel: ctx.channel }
    if (gesture.provisional) {
      const inits = movedCopies(notes, delta)
      this.selected = new Set()
      if (!initsFit(inits)) return this.refusePastEnd()
      this.commitDrag(
        withExtension(
          noteInsertionCommand(target, inits, curveInserts(notes, ctx.pattern.noteCurves)),
          "Add note",
          ctx.pattern,
          endOfInits(inits)
        )
      )
      return
    }
    if (delta.ticks === 0 && delta.keys === 0) {
      this.clearPreview()
      if (gesture.moved) return
      // A click, not a drag: it only changes what is selected.
      if (gesture.shift && gesture.wasSelected) {
        this.toggleSelected(gesture.grabbed.id)
      } else if (!gesture.shift && this.selected.size > 1) {
        this.setSelection([gesture.grabbed.id])
      }
      return
    }
    if (input.ctrl) {
      const inits = movedCopies(notes, delta)
      if (!initsFit(inits)) return this.refusePastEnd()
      this.commitDrag(
        withExtension(
          noteInsertionCommand(target, inits, curveInserts(notes, ctx.pattern.noteCurves)),
          "Duplicate notes",
          ctx.pattern,
          endOfInits(inits)
        ),
        true
      )
      return
    }
    const updates = moveUpdates(notes, delta)
    if (!updatesFit(notes, updates)) return this.refusePastEnd()
    this.commitDrag(
      withExtension(
        { type: "updateNotes", ...target, updates },
        "Move notes",
        ctx.pattern,
        endAfterUpdates(notes, updates)
      )
    )
  }

  private beginResize(
    hit: Hit,
    edge: ResizeEdge,
    input: PointerInput,
    press: Press,
    tick: number
  ): void {
    const grabbed = this.scene.notes[hit.index]
    if (!this.selected.has(grabbed.id)) {
      this.setSelection(
        input.shift ? [...this.selected, grabbed.id] : [grabbed.id]
      )
    }
    const notes = this.selectedNotes()
    this.gesture = {
      kind: "resize",
      press,
      tick,
      edge,
      grabbed,
      notes,
      delta: { start: 0, end: 0, minLength: 1 },
      moved: false,
    }
  }

  private beginDraw(
    input: PointerInput,
    press: Press,
    tick: number,
    row: number,
    settings: EditorSettings
  ): void {
    const rawKey = rowToKey(Math.floor(row))
    if (rawKey < 0 || rawKey > MAX_KEY) return
    const key =
      !noSnap(input) && settings.pitchScale
        ? (nearestScaleKey(rawKey, settings.pitchScale) ?? rawKey)
        : rawKey
    const snap = noSnap(input) ? 0 : settings.snap
    const note: Note = {
      id: PROVISIONAL_ID,
      start: Math.max(0, snapFloor(tick, snap, settings.snapOrigin)),
      length: Math.max(1, settings.lastLength),
      key,
      velocity: settings.lastVelocity,
      pan: 0,
      expression: drawnExpression(settings),
    }
    this.provisional = [note]
    this.selected = new Set([note.id])
    this.rebuild()
    this.emit("selection")
    this.gesture = {
      kind: "move",
      press,
      tick,
      row,
      grabbed: note,
      notes: [note],
      limits: moveLimits([note]),
      delta: { ticks: 0, keys: 0 },
      wasSelected: false,
      shift: false,
      provisional: true,
      moved: false,
    }
    if (note.expression?.articulation !== "slide") this.sound(key, note.velocity)
  }

  /** Drum cells follow the snap grid, including local meter origins. */
  private drumAlong(
    gesture: Extract<Gesture, { kind: "drum" }>,
    tick: number,
    row: number
  ): void {
    const ctx = this.ctx
    if (!ctx) return
    const segments = meterSegments(ctx.pattern.signature, ctx.pattern.timeline?.meters ?? [])
    for (const span of rowsAlongSegment(gesture.tick, gesture.row, tick, row)) {
      const key = rowToKey(span.row)
      if (key < 0 || key > MAX_KEY) continue
      for (const segment of segments) {
        if (segment.start > span.tick1 || segment.end <= span.tick0) continue
        const settings = this.host.settings(segment.start)
        const spacing = paintSpacing(
          settings.snap,
          settings.snap > 0 ? settings.snap : gesture.fallbackLength
        )
        const first = Math.max(segment.start, span.tick0)
        const last = Math.min(MAX_PATTERN_TICKS, segment.end - 1, span.tick1)
        if (first > last) continue
        const cell0 = Math.floor((first - segment.start) / spacing)
        const cell1 = Math.floor((last - segment.start) / spacing)
        for (const cell of cellsBetween(cell0, cell1)) {
          const start = segment.start + cell * spacing
          const length = Math.min(spacing, segment.end - start)
          if (start < 0 || start > MAX_PATTERN_TICKS) continue
          if (start + length > MAX_PATTERN_TICKS) {
            gesture.pastEnd = true
            continue
          }
          visitDrumCell(gesture.stroke, ctx.notes, {
            start,
            length,
            key,
            velocity: gesture.velocity,
            pan: 0,
            expression: gesture.expression ? { ...gesture.expression } : undefined,
          })
        }
      }
    }
    gesture.tick = tick
    gesture.row = row
    this.provisional = gesture.stroke.adds.map((note, index) => ({
      ...note,
      id: PROVISIONAL_ID - index,
      velocity: note.velocity ?? gesture.velocity,
      pan: note.pan ?? 0,
    }))
    this.rebuild()
  }

  private beginPaint(
    input: PointerInput,
    tick: number,
    row: number,
    settings: EditorSettings
  ): void {
    const snap = noSnap(input) ? 0 : settings.snap
    const gesture: Extract<Gesture, { kind: "paint" }> = {
      kind: "paint",
      origin: Math.max(0, snapFloor(tick, snap, settings.snapOrigin)),
      spacing: paintSpacing(snap, settings.lastLength),
      length: Math.max(1, settings.lastLength),
      velocity: settings.lastVelocity,
      expression: drawnExpression(settings),
      cells: new Map(),
      lastCell: 0,
      lastTick: tick,
      pastEnd: false,
    }
    this.gesture = gesture
    if (this.selected.size > 0) this.setSelection([])
    this.paintCells(gesture, [gesture.origin], row, input)
  }

  private meterPaintStarts(gesture: Extract<Gesture, { kind: "paint" }>, from: number, to: number): number[] {
    const pattern = this.ctx!.pattern
    const low = Math.max(0, Math.min(from, to))
    const high = Math.min(MAX_PATTERN_TICKS, Math.max(from, to))
    const starts: number[] = []
    for (const segment of meterSegments(pattern.signature, pattern.timeline?.meters ?? [])) {
      if (segment.start > high || segment.end <= low) continue
      const settings = this.host.settings(segment.start)
      const spacing = paintSpacing(settings.snap, gesture.length)
      const origin = gesture.origin >= segment.start && gesture.origin < segment.end ? gesture.origin : segment.start
      const first = Math.max(segment.start, low)
      const last = Math.min(high, segment.end - 1)
      const cell0 = Math.floor((first - origin) / spacing)
      const cell1 = Math.floor((last - origin) / spacing)
      for (let cell = cell0; cell <= cell1 && starts.length < 16384; cell++) {
        const start = origin + cell * spacing
        if (start >= segment.start && start < segment.end) starts.push(start)
      }
    }
    return to < from ? starts.reverse() : starts
  }

  private paintCells(
    gesture: Extract<Gesture, { kind: "paint" }>,
    starts: readonly number[],
    row: number,
    input: PointerInput
  ): void {
    const rawKey = rowToKey(Math.floor(row))
    if (rawKey < 0 || rawKey > MAX_KEY) return
    const scale = this.host.settings().pitchScale
    const key =
      !noSnap(input) && scale
        ? (nearestScaleKey(rawKey, scale) ?? rawKey)
        : rawKey
    const keyRow = keyToRow(key)
    let added = false
    for (const start of starts) {
      if (gesture.cells.has(start)) continue
      if (start < 0) continue
      if (start + gesture.length > MAX_PATTERN_TICKS) {
        gesture.pastEnd = true
        continue
      }
      // A cell that already holds a note on this key is left alone.
      const taken = queryRect(
        this.scene.items,
        start,
        start + 1,
        keyRow,
        keyRow + 1
      )
      if (taken.length > 0) continue
      gesture.cells.set(start, {
        id: PROVISIONAL_ID - gesture.cells.size,
        start,
        length: gesture.length,
        key,
        velocity: gesture.velocity,
        pan: 0,
        expression: gesture.expression ? { ...gesture.expression } : undefined,
      })
      added = true
    }
    if (!added) return
    this.provisional = [...gesture.cells.values()]
    this.rebuild()
    this.sound(null)
    if (gesture.expression?.articulation !== "slide") this.sound(key, gesture.velocity)
  }

  private markErased(indices: readonly number[]): void {
    const gesture = this.gesture
    if (gesture.kind !== "erase") return
    const batch = this.scene.items.batch
    let changed = false
    for (const index of indices) {
      const id = batch.ids[index]
      if (gesture.ids.has(id)) continue
      gesture.ids.add(id)
      const c = index * 4
      batch.setColor(
        index,
        withAlpha(
          {
            r: batch.colors[c],
            g: batch.colors[c + 1],
            b: batch.colors[c + 2],
            a: batch.colors[c + 3],
          },
          ERASED_ALPHA
        )
      )
      changed = true
    }
    if (!changed) return
    this.surface?.invalidate("base")
    this.emit("drag")
  }

  private eraseAlong(
    tick0: number,
    row0: number,
    tick1: number,
    row1: number
  ): void {
    for (const span of rowsAlongSegment(tick0, row0, tick1, row1)) {
      // A pointer that has not moved covers no time, so give it a sliver.
      const end = Math.max(span.tick1, span.tick0 + 0.001)
      this.markErased(
        queryRect(this.scene.items, span.tick0, end, span.row, span.row + 1)
      )
    }
  }
}

function pastDeadZone(press: Press, input: PointerInput): boolean {
  return (
    Math.abs(input.x - press.x) > DEAD_ZONE_PX ||
    Math.abs(input.y - press.y) > DEAD_ZONE_PX
  )
}

function intentName(intent: Intent | null): string {
  if (!intent) return ""
  return intent.kind === "resize" ? `resize-${intent.edge}` : intent.kind
}
