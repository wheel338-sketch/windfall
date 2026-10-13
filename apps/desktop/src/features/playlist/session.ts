import type {
  AutomationId,
  AutomationPoint,
  Clip,
  ClipId,
  ClipInit,
  ClipUpdate,
} from "@/bindings"
import { formatGain } from "@/components/audio"
import {
  deviceX,
  queryRect,
  rgbaToCss,
  snapTick,
  xToTick,
  type Hit,
  type OverlayPainter,
} from "@/lib/canvas"
import {
  extendView,
  FULL_VIEW,
  type ViewRange,
} from "@/lib/automation/view-range"
import { ignoresSnap } from "@/lib/edit-modifiers"
import { refuse } from "@/lib/errors"
import { dispatch } from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { seek } from "@/lib/store/transport"
import { ticksPerBar } from "@/lib/time"
import { meterSegments } from "@/lib/timeline"
import {
  clamp,
  MAX_AUTOMATION_POINTS,
  MAX_SONG_TICKS,
  TICKS_PER_STEP,
} from "@/lib/units"

import { describeFade, fadeFromPointer, gainFromDrag } from "./audio/geometry"
import {
  changeAudioClips,
  mixerTrackForSample,
  placeSampleClips,
} from "./audio/ops"
import { sampleDuration } from "./audio/peaks"
import { formatAutomationValue, formatBarBeat } from "./automation/format"
import { valueAt, valueBeyond, type CurveView } from "./automation/hit"
import { deletePointAt, setCurve, togglePointHold } from "./automation/ops"
import { showInView } from "./automation/view-store"
import { writeSteps, type StepSample } from "./automation/step"
import {
  bendSegment,
  bendThrough,
  constrainToAxis,
  insertPoint,
  movePoint,
  moveSelectedPoints,
  toCurveTick,
  toSongTick,
} from "./automation/points"
import { brushClip, brushTicks, type BrushContext } from "./brush"
import { clipGroupMove, expandClipSelection } from "./clip-groups"
import {
  clampMove,
  cloneMoved,
  endResizeDelta,
  minResizeLength,
  moveChanges,
  paintStarts,
  resizeChanges,
  snapCell,
  snapNearest,
  startTrimDelta,
  strokeBox,
  withoutStacked,
  type NewClip,
} from "./edit"
import { curveViewOf, type InnerHit } from "./inner"
import { cursorFor, intentFor } from "./intents"
import {
  DOUBLE_CLICK_MS,
  DOUBLE_CLICK_SLOP_PX,
  DRAG_THRESHOLD_PX,
  edgePull,
} from "./layout"
import type { GridMetrics } from "./metrics"
import {
  addClips,
  changeClips,
  currentSnapTicks,
  deleteClips,
  openPattern,
  setClipsMuted,
} from "./ops"
import {
  NO_DRAG,
  type AudioContent,
  type ClipPainter,
  type DropPreview,
  type PointSelection,
} from "./painter"
import { playlistYToRow } from "./row-geometry"
import { PlaylistScene } from "./scene"
import { documentRowFor, trackRowsNow } from "./track-rows"
import {
  project,
  resolveBrush,
  selectedClips,
  type ResolvedBrush,
} from "./selectors"
import { usePlaylistStore } from "./store"
import { snapTicks } from "./snap"
import type { GridSurface } from "./surface"
import { translatedText as t } from "@/lib/i18n"

/** A pointer event, already placed inside the grid. */
export type PointerInput = {
  /** CSS pixels from the grid's top left corner. */
  x: number
  y: number
  button: number
  shift: boolean
  /** Ctrl, or Cmd on macOS. */
  mod: boolean
  alt: boolean
}

type Point = { tick: number; row: number }

/** Pixels of pointer travel that take a bend from straight to full. */
const BEND_DRAG_PX = 80

type Gesture =
  | { kind: "idle" }
  | { kind: "pan"; x: number; y: number }
  | { kind: "playback" }
  | { kind: "slice"; tick: number }
  | {
      kind: "marquee"
      from: Point
      base: ReadonlySet<ClipId>
      ids: Set<ClipId>
    }
  | {
      kind: "move"
      anchor: Clip
      clips: Clip[]
      from: Point
      x: number
      y: number
      dragging: boolean
      /** The clip was selected before the press, so a Shift+click drops it. */
      wasSelected: boolean
      additive: boolean
      grouped: boolean
      updates: ClipUpdate[] | null
    }
  | {
      kind: "resize"
      edge: "start" | "end"
      anchor: Clip
      clips: Clip[]
      x: number
      dragging: boolean
    }
  | {
      kind: "slip"
      clip: Clip
      fromTick: number
      x: number
      offset: number
      dragging: boolean
    }
  | { kind: "place"; brush: ResolvedBrush }
  | { kind: "paint"; brush: ResolvedBrush; anchor: number; row: number }
  | {
      kind: "stroke"
      action: "erase" | "mute"
      /** What a mute stroke sets: the opposite of the first clip it touches. */
      muteTo: boolean | null
      last: Point
    }
  /** The end of an audio clip's fade is being dragged. */
  | {
      kind: "fade"
      edge: "in" | "out"
      clip: Clip
      content: AudioContent
      ticks: number
      x: number
      dragging: boolean
    }
  /** An audio clip's gain is being dragged up or down. */
  | {
      kind: "gain"
      clip: Clip
      content: AudioContent
      gain: number
      y: number
      dragging: boolean
    }
  /** A point of a curve is being dragged. */
  | {
      kind: "point"
      clip: Clip
      automation: AutomationId
      /** The curve the drag started from, with the point in it. */
      base: readonly AutomationPoint[]
      points: readonly AutomationPoint[]
      index: number
      indices: ReadonlySet<number>
      /** A plain click collapses the selection only if it did not drag. */
      selectOnlyOnClick: boolean
      x: number
      y: number
      dragging: boolean
      /** The press put the point there, so letting go without moving keeps it. */
      added: boolean
      /** The view the curve was drawn in at the press, which the drag keeps. */
      range: ViewRange
    }
  /** A curve-body stroke writes held points on the grid. */
  | {
      kind: "step"
      clip: Clip
      automation: AutomationId
      points: readonly AutomationPoint[]
      last: StepSample
      spacing: number
      range: ViewRange
    }
  /** The stretch of a curve leaving a point is being bent. */
  | {
      kind: "bend"
      clip: Clip
      automation: AutomationId
      base: readonly AutomationPoint[]
      points: readonly AutomationPoint[]
      index: number
      y: number
      dragging: boolean
      range: ViewRange
    }
  /** The button is up and the edit is on its way to the project. */
  | { kind: "committing" }

const ui = () => usePlaylistStore.getState()
const NOTHING: ReadonlySet<ClipId> = new Set()

const plural = (count: number, one: string) => (count === 1 ? one : `${one}s`)

/**
 * Turns the pointer on the playlist's grid into edits. It talks to the
 * canvas through `GridSurface` and takes pointer input as plain numbers, so
 * all of it runs without a canvas or a DOM.
 *
 * Nothing is sent to the project while a button is down. A drag shows its
 * result by offsetting the selection on the canvas and by drawing the clips
 * to come on the overlay; the edit goes out once, when the button comes up.
 */
export class PlaylistSession {
  /** Called when the mouse cursor over the grid should change. */
  onCursor: (cursor: string) => void = () => {}
  /**
   * Called when the pointer comes to rest on, or leaves, a part of a clip
   * with an edit of its own, so the status bar can say what it does.
   */
  onInner: (part: InnerHit["kind"] | null) => void = () => {}
  private innerShown: InnerHit["kind"] | null = null

  private readonly surface: GridSurface
  private readonly metrics: GridMetrics
  private readonly scene: PlaylistScene
  private readonly now: () => number
  private readonly removeSlicePainter: () => void
  private readonly stopProjectSelection: () => void
  private gesture: Gesture = { kind: "idle" }
  private lastPress: {
    id: ClipId
    /** The point of a curve that was pressed, or -1 for the clip itself. */
    point: number
    time: number
    x: number
    y: number
  } | null = null
  private pressedAt: { x: number; y: number } | null = null
  private lastInput: PointerInput | null = null
  private scrollFrame = 0

  constructor(
    surface: GridSurface,
    metrics: GridMetrics,
    now: () => number = () => performance.now()
  ) {
    this.surface = surface
    this.metrics = metrics
    this.now = now
    this.scene = new PlaylistScene(surface, metrics)
    this.removeSlicePainter = surface.addOverlayPainter(this.paintSlice)
    this.stopProjectSelection = onProjectReplaced(() => {
      this.settle()
      this.scene.setPointSelection(null)
      this.lastPress = null
    })
    // The edit a drop sent has arrived, so what the drag was showing is now
    // the real thing. Dropping the preview in the same turn as the canvas
    // is rebuilt keeps the picture from jumping.
    this.scene.onClipsChanged = () => {
      if (this.gesture.kind === "committing") this.scene.clearPreview()
    }
  }

  destroy(): void {
    this.stopEdgeScroll()
    this.removeSlicePainter()
    this.stopProjectSelection()
    this.scene.destroy()
  }

  /** The vertical cut being positioned, before any clips have changed. */
  get slicePreviewTick(): number | null {
    return this.gesture.kind === "slice" ? this.gesture.tick : null
  }

  private paintSlice: OverlayPainter = (ctx, { transform, theme }) => {
    const tick = this.slicePreviewTick
    if (tick === null) return
    const x = Math.round(deviceX(transform, tick))
    ctx.fillStyle = rgbaToCss(theme.item)
    ctx.fillRect(x, 0, transform.lineWidth, transform.heightDev)
  }

  /** True while a button is down on the grid or an edit is being sent. */
  get busy(): boolean {
    return this.gesture.kind !== "idle"
  }

  /** The clips that will exist once the button is released. */
  get ghosts(): readonly NewClip[] {
    return this.scene.ghosts
  }

  /** The clips an erase or mute stroke has crossed so far. */
  get marked(): ReadonlySet<ClipId> {
    return this.scene.marked
  }

  get painter(): ClipPainter {
    return this.scene.painter
  }

  /** The curve a drag is showing, before it has gone to the project. */
  get draftPoints(): readonly AutomationPoint[] | null {
    return this.scene.draft?.points ?? null
  }

  get pointSelection(): PointSelection | null {
    return this.scene.pointSelection
  }

  /** What the box beside the pointer says while something is dragged. */
  get badgeText(): string | null {
    return this.scene.badge?.text ?? null
  }

  /** Where a file dragged over the grid would land. */
  get dropPreview(): DropPreview | null {
    return this.scene.dropPreview
  }

  private settle(): void {
    const slicing = this.gesture.kind === "slice"
    this.gesture = { kind: "idle" }
    this.stopEdgeScroll()
    this.scene.clearPreview()
    if (slicing) this.surface.invalidate()
  }

  private pointAt(input: { x: number; y: number }): Point {
    const viewport = this.surface.viewport
    return {
      tick: xToTick(viewport, input.x),
      row: playlistYToRow(viewport, input.y),
    }
  }

  private rowAt(point: Point): number {
    return clamp(Math.floor(point.row), 0, this.surface.limits.rowCount - 1)
  }

  private snapFor(input: { alt: boolean }): number {
    return ignoresSnap(input) ? 0 : currentSnapTicks()
  }

  /** The cut follows the visible grid and each meter segment's origin. */
  private sliceTick(tick: number, input: { alt: boolean }): number {
    const { settings, playlist } = project()
    const segments = meterSegments(
      settings.timeSignature,
      playlist.timeline?.meters ?? []
    )
    const segment =
      segments.findLast((segment) => segment.start <= tick) ?? segments[0]
    const snap = ignoresSnap(input)
      ? 0
      : snapTicks(ui().snap, segment.signature)
    return clamp(
      segment.start + snapNearest(tick - segment.start, snap),
      0,
      segment.end
    )
  }

  /** Scrubbing lands on the visible grid, including each meter's origin. */
  private playbackTick(tick: number, input: { alt: boolean }): number {
    const { settings, playlist } = project()
    const segments = meterSegments(
      settings.timeSignature,
      playlist.timeline?.meters ?? []
    )
    const segment =
      segments.findLast((segment) => segment.start <= tick) ?? segments[0]
    const snap = ignoresSnap(input)
      ? 0
      : snapTicks(ui().snap, segment.signature)
    return clamp(
      segment.start + snapNearest(tick - segment.start, snap),
      0,
      segment.end
    )
  }

  /** The part of a clip under the pointer that has an edit of its own. */
  private innerAt(
    hit: { id: ClipId } | null,
    input: PointerInput
  ): InnerHit | null {
    const clip = hit ? this.scene.clipById.get(hit.id) : undefined
    return clip ? this.scene.innerAt(clip, input.x, input.y) : null
  }

  /**
   * What the pointer is on. The points and bend handles of curves come
   * before the clips and the empty grid, also where a dot hangs over the
   * edge of its clip: there the press is on the point, and neither on the
   * clip beside it nor on empty grid, where the Draw tool would place a
   * clip. Only the tools that edit clips look, and not with Ctrl held,
   * which makes every press the clip's.
   */
  private targetAt(input: PointerInput): {
    hit: Hit | null
    inner: InnerHit | null
  } {
    const hit = this.scene.hitAt(input.x, input.y)
    const tool = ui().tool
    const edits = tool === "draw" || tool === "paint" || tool === "select"
    if (!edits) return { hit, inner: null }
    const mark = input.mod ? null : this.scene.curveMarkAt(input.x, input.y)
    if (mark) {
      // Inside its own clip the press keeps the part it is on, so a bend
      // handle still leaves the clip's edges to resizing.
      const id = mark.clip.id
      const own = hit?.id === id ? hit : null
      const index = this.scene.items?.batch.indexOfId(id) ?? -1
      return { hit: own ?? { index, id, part: "body" }, inner: mark.inner }
    }
    return { hit, inner: this.innerAt(hit, input) }
  }

  /**
   * How a clip's curve lies on screen now. `range` is the view to lay it
   * out in; left out, the one its automation is drawn in.
   */
  private curveView(clip: Clip, range?: ViewRange): CurveView | null {
    const automation =
      clip.content.type === "automation" ? clip.content.automation : null
    return curveViewOf(
      this.surface.viewport,
      clip,
      this.scene.rowOf(clip.track),
      range ?? (automation === null ? undefined : this.scene.viewOf(automation))
    )
  }

  /** A button went down. Returns true when the drag should keep the pointer. */
  pointerDown(input: PointerInput): boolean {
    if (this.gesture.kind !== "idle") return false
    this.pressedAt = { x: input.x, y: input.y }
    const { tool, selection } = ui()
    const scene = this.scene
    const { hit, inner } = this.targetAt(input)
    const intent = intentFor({
      tool,
      button: input.button,
      mod: input.mod,
      shift: input.shift,
      hit,
      inner,
    })
    const point = this.pointAt(input)

    const pressed =
      "id" in intent && intent.id !== null
        ? scene.clipById.get(intent.id)
        : undefined
    // The presses a second one of which, on the same spot, is a double-click.
    const clicks =
      intent.kind === "move" ||
      intent.kind === "trim-start" ||
      intent.kind === "resize-end" ||
      (intent.kind === "point" && !input.shift)
    if (pressed && clicks) {
      const onPoint = intent.kind === "point" ? intent.index : -1
      if (this.isDoubleClick(pressed.id, onPoint, input)) {
        // Twice on a point makes it a step; twice on a clip opens its
        // pattern.
        const content = pressed.content
        if (intent.kind === "point" && content.type === "automation") {
          void togglePointHold({
            clip: pressed.id,
            automation: content.automation,
            index: intent.index,
          })
        } else if (intent.kind !== "point" && content.type === "pattern") {
          void openPattern(content.pattern)
        }
        return false
      }
    } else {
      this.lastPress = null
    }
    if (intent.kind !== "point-menu") ui().setMenuPoint(null)

    switch (intent.kind) {
      case "none":
        return false
      case "playback":
        this.gesture = { kind: "playback" }
        this.onCursor("crosshair")
        this.pointerMove(input)
        return true
      case "slice":
        this.gesture = {
          kind: "slice",
          tick: this.sliceTick(point.tick, input),
        }
        this.onCursor("crosshair")
        this.surface.invalidate()
        return true
      case "pan":
        this.gesture = { kind: "pan", x: input.x, y: input.y }
        this.onCursor("grabbing")
        return true
      case "menu": {
        if (intent.id !== null) {
          ui().select(
            expandClipSelection(
              selection.has(intent.id) ? selection : [intent.id],
              project().playlist.arrangementBook?.clipGroups ?? []
            )
          )
        }
        ui().setMenuOnClips(intent.id !== null)
        return false
      }
      case "point-menu": {
        if (pressed?.content.type !== "automation") return false
        if (!selection.has(pressed.id)) ui().select([pressed.id])
        ui().setMenuOnClips(true)
        ui().setMenuPoint({
          clip: pressed.id,
          automation: pressed.content.automation,
          index: intent.index,
        })
        return false
      }
      case "delete-point": {
        if (pressed?.content.type !== "automation") return false
        void deletePointAt({
          clip: pressed.id,
          automation: pressed.content.automation,
          index: intent.index,
        })
        return false
      }
      case "marquee": {
        const base = intent.additive ? selection : NOTHING
        if (!intent.additive) ui().clearSelection()
        this.gesture = {
          kind: "marquee",
          from: point,
          base,
          ids: new Set(base),
        }
        return true
      }
      case "move": {
        if (!pressed) return false
        const wasSelected = selection.has(pressed.id)
        const groups = project().playlist.arrangementBook?.clipGroups ?? []
        const ids = expandClipSelection(
          wasSelected || intent.additive
            ? [...selection, pressed.id]
            : [pressed.id],
          groups
        )
        ui().select(ids)
        const grouped = groups.some((group) =>
          group.clips.some((id) => ids.includes(id))
        )
        this.gesture = {
          kind: "move",
          anchor: pressed,
          clips: grouped
            ? ids.flatMap((id) => scene.clipById.get(id) ?? [])
            : selectedClips(),
          from: { tick: point.tick, row: Math.floor(point.row) },
          x: input.x,
          y: input.y,
          dragging: false,
          wasSelected,
          additive: intent.additive,
          grouped,
          updates: null,
        }
        return true
      }
      case "trim-start":
      case "resize-end": {
        if (!pressed) return false
        if (!selection.has(pressed.id)) ui().select([pressed.id])
        this.gesture = {
          kind: "resize",
          edge: intent.kind === "trim-start" ? "start" : "end",
          anchor: pressed,
          clips: selectedClips(),
          x: input.x,
          dragging: false,
        }
        return true
      }
      case "slip": {
        if (!pressed) return false
        if (!selection.has(pressed.id)) ui().select([pressed.id])
        this.gesture = {
          kind: "slip",
          clip: pressed,
          fromTick: point.tick,
          x: input.x,
          offset: pressed.offset,
          dragging: false,
        }
        this.onCursor("ew-resize")
        return true
      }
      case "fade":
      case "gain": {
        const content = pressed?.content
        if (!pressed || content?.type !== "audio") return false
        if (!selection.has(pressed.id)) ui().select([pressed.id])
        this.gesture =
          intent.kind === "fade"
            ? {
                kind: "fade",
                edge: intent.edge,
                clip: pressed,
                content,
                ticks: intent.edge === "in" ? content.fadeIn : content.fadeOut,
                x: input.x,
                dragging: false,
              }
            : {
                kind: "gain",
                clip: pressed,
                content,
                gain: content.gain,
                y: input.y,
                dragging: false,
              }
        this.onCursor(intent.kind === "fade" ? "ew-resize" : "ns-resize")
        return true
      }
      case "point":
      case "bend": {
        const content = pressed?.content
        if (!pressed || content?.type !== "automation") return false
        const base = scene.pointsOf(content.automation)
        if (!base) return false
        if (!selection.has(pressed.id)) ui().select([pressed.id])
        let indices: ReadonlySet<number> = new Set([intent.index])
        if (intent.kind === "point") {
          const current = scene.pointSelection
          const sameClip =
            current?.clip === pressed.id &&
            current.automation === content.automation
          const wasSelected = sameClip && current.indices.has(intent.index)
          if (input.shift) {
            const toggled = new Set(sameClip ? current.indices : [])
            if (wasSelected) toggled.delete(intent.index)
            else toggled.add(intent.index)
            indices = toggled
          } else if (wasSelected) {
            indices = current.indices
          }
          scene.setPointSelection({
            clip: pressed.id,
            automation: content.automation,
            indices,
          })
          if (!indices.has(intent.index)) return false
        }
        const held = {
          clip: pressed,
          automation: content.automation,
          base,
          points: base,
          index: intent.index,
          y: input.y,
          dragging: false,
          range: scene.viewOf(content.automation),
        }
        this.gesture =
          intent.kind === "point"
            ? {
                kind: "point",
                ...held,
                indices,
                selectOnlyOnClick: !input.shift,
                x: input.x,
                added: false,
              }
            : { kind: "bend", ...held }
        return true
      }
      case "add-point": {
        const content = pressed?.content
        if (!pressed || content?.type !== "automation") return false
        const base = scene.pointsOf(content.automation)
        const view = this.curveView(pressed)
        if (!base || !view) return false
        const window = view.window
        if (ui().step) {
          if (base.length >= MAX_AUTOMATION_POINTS) {
            this.refuseFullCurve()
            return false
          }
          const spacing = Math.max(1, this.snapFor(input))
          const sample = this.stepSample(view, input, spacing)
          if (!selection.has(pressed.id)) ui().select([pressed.id])
          this.gesture = {
            kind: "step",
            clip: pressed,
            automation: content.automation,
            points: base,
            last: sample,
            spacing,
            range: view.range ?? FULL_VIEW,
          }
          scene.setPointSelection(null)
          this.onCursor("crosshair")
          this.pointerMove(input)
          return true
        }
        const songTick = snapNearest(
          xToTick(this.surface.viewport, input.x),
          this.snapFor(input)
        )
        const added = insertPoint(
          base,
          clamp(
            toCurveTick(window, songTick),
            window.offset,
            window.offset + window.length
          ),
          valueAt(view, input.y)
        )
        if (!added) {
          refuse(
            t("This curve is full"),
            t("A curve holds 4,096 points. Delete some to add more.")
          )
          return false
        }
        if (!selection.has(pressed.id)) ui().select([pressed.id])
        this.gesture = {
          kind: "point",
          clip: pressed,
          automation: content.automation,
          base: added.points,
          points: added.points,
          index: added.index,
          indices: new Set([added.index]),
          selectOnlyOnClick: false,
          x: input.x,
          y: input.y,
          dragging: false,
          added: true,
          range: view.range ?? FULL_VIEW,
        }
        scene.setPointSelection(null)
        scene.setDraft({ automation: content.automation, points: added.points })
        this.showPointBadge(input, pressed, added.points, added.index)
        this.onCursor("move")
        return true
      }
      case "place": {
        const brush = resolveBrush()
        if (!brush) return false
        ui().clearSelection()
        this.gesture = { kind: "place", brush }
        this.pointerMove(input)
        return true
      }
      case "paint": {
        const brush = resolveBrush()
        if (!brush) return false
        ui().clearSelection()
        this.gesture = {
          kind: "paint",
          brush,
          anchor: snapCell(point.tick, this.snapFor(input)),
          row: this.rowAt(point),
        }
        this.pointerMove(input)
        return true
      }
      case "erase":
      case "mute": {
        // The clip under the press counts even when it is a sliver that
        // the stroke's own box would miss.
        const first = hit ? scene.clipById.get(hit.id) : undefined
        this.gesture = {
          kind: "stroke",
          action: intent.kind,
          muteTo: first ? !first.muted : null,
          last: point,
        }
        const ids = first ? [first.id] : []
        scene.setMarked(
          new Set(
            intent.kind === "erase"
              ? expandClipSelection(
                  ids,
                  project().playlist.arrangementBook?.clipGroups ?? []
                )
              : ids
          )
        )
        this.pointerMove(input)
        return true
      }
      default: {
        const _exhaustive: never = intent
        return _exhaustive
      }
    }
  }

  /**
   * Two quick presses on the same spot of a clip, or on the same point of
   * its curve. A click on a clip and then a grab of its edge is not one, or
   * resizing a clip that was just selected would open its pattern instead.
   */
  private isDoubleClick(
    id: ClipId,
    point: number,
    input: PointerInput
  ): boolean {
    const time = this.now()
    const last = this.lastPress
    if (
      last &&
      last.id === id &&
      last.point === point &&
      time - last.time <= DOUBLE_CLICK_MS &&
      Math.hypot(input.x - last.x, input.y - last.y) <= DOUBLE_CLICK_SLOP_PX
    ) {
      this.lastPress = null
      return true
    }
    this.lastPress = { id, point, time, x: input.x, y: input.y }
    return false
  }

  /** What a brush's clip is measured by, as things are now. */
  private brushContext(brush: ResolvedBrush, tick: number): BrushContext {
    const { settings, playlist } = project()
    const segments = meterSegments(
      settings.timeSignature,
      playlist.timeline?.meters ?? []
    )
    const segment =
      segments.findLast((segment) => segment.start <= tick) ?? segments[0]
    return {
      barTicks: ticksPerBar(segment.signature),
      tempoBpm: settings.tempoBpm,
      durationSecs:
        brush.type === "audio" ? sampleDuration(brush.sample) : null,
      mixerTrack:
        brush.type === "audio"
          ? mixerTrackForSample(brush.sample.id)
          : undefined,
    }
  }

  /** Says where a dragged point is: its value in real units, and its place. */
  private showPointBadge(
    input: PointerInput,
    clip: Clip,
    points: readonly AutomationPoint[],
    index: number
  ): void {
    const point = points[index]
    const current = project()
    const automation =
      clip.content.type === "automation"
        ? this.scene.lookups.automations.get(clip.content.automation)
        : undefined
    if (!point || !automation) return
    const value = formatAutomationValue(current, automation.target, point.value)
    const place = formatBarBeat(
      toSongTick(clip, point.tick),
      current.settings.timeSignature
    )
    this.scene.setBadge({
      x: input.x,
      y: input.y,
      text: `${value}  ·  ${place}${point.hold ? "  ·  hold" : ""}`,
    })
  }

  private refuseFullCurve(): void {
    refuse(
      t("This curve is full"),
      t("A curve holds 4,096 points. Delete some to add more.")
    )
  }

  private stepSample(
    view: CurveView,
    input: PointerInput,
    spacing: number
  ): StepSample {
    const songTick = snapNearest(
      xToTick(this.surface.viewport, input.x),
      spacing
    )
    return {
      tick: Math.round(
        clamp(
          toCurveTick(view.window, songTick),
          Math.max(0, view.window.offset),
          Math.min(MAX_SONG_TICKS, view.window.offset + view.window.length)
        )
      ),
      value: valueAt(view, input.y),
    }
  }

  /** The pointer moved, with or without a button down. */
  pointerMove(input: PointerInput): void {
    const gesture = this.gesture
    const scene = this.scene
    const point = this.pointAt(input)
    switch (gesture.kind) {
      case "idle": {
        const { hit, inner } = this.targetAt(input)
        const tool = ui().tool
        const edits = tool === "draw" || tool === "paint" || tool === "select"
        // An automation clip shows its bend handles while it is pointed at.
        const over = hit ? scene.clipById.get(hit.id) : undefined
        scene.setHover(
          edits && over?.content.type === "automation" ? over.id : null
        )
        this.onCursor(cursorFor(tool, hit?.part ?? null, inner))
        this.showInner(inner?.kind ?? null)
        return
      }
      case "committing":
        return
      case "playback":
        void seek(this.playbackTick(point.tick, input))
        return
      case "slice":
        gesture.tick = this.sliceTick(point.tick, input)
        this.surface.invalidate()
        break
      case "pan":
        this.metrics.panBy(gesture.x - input.x, gesture.y - input.y)
        gesture.x = input.x
        gesture.y = input.y
        return
      case "fade": {
        if (!gesture.dragging) {
          if (Math.abs(input.x - gesture.x) < DRAG_THRESHOLD_PX) return
          gesture.dragging = true
        }
        // A fade is short next to a bar, so it lands on sixteenths at the
        // coarsest, whatever the clips snap to.
        const snap = this.snapFor(input)
        const ticks = fadeFromPointer(
          gesture.edge,
          gesture.clip,
          point.tick,
          snap > 0 ? Math.min(snap, TICKS_PER_STEP) : 0
        )
        gesture.ticks = ticks
        const content: AudioContent =
          gesture.edge === "in"
            ? { ...gesture.content, fadeIn: ticks }
            : { ...gesture.content, fadeOut: ticks }
        scene.setAudioDraft({ clip: gesture.clip.id, content })
        scene.setBadge({
          x: input.x,
          y: input.y,
          text: `Fade ${gesture.edge}: ${describeFade(ticks, scene.tempoBpm)}`,
        })
        return
      }
      case "gain": {
        if (!gesture.dragging) {
          if (Math.abs(input.y - gesture.y) < DRAG_THRESHOLD_PX) return
          gesture.dragging = true
        }
        gesture.gain = gainFromDrag(
          gesture.content.gain,
          gesture.y - input.y,
          input.shift
        )
        scene.setAudioDraft({
          clip: gesture.clip.id,
          content: { ...gesture.content, gain: gesture.gain },
        })
        scene.setBadge({
          x: input.x,
          y: input.y,
          text: `Gain: ${formatGain(gesture.gain)}`,
        })
        return
      }
      case "step": {
        const view = this.curveView(gesture.clip, gesture.range)
        if (!view) return
        const sample = this.stepSample(view, input, gesture.spacing)
        const points = writeSteps(
          gesture.points,
          gesture.last,
          sample,
          gesture.spacing,
          view.window.offset - view.window.start
        )
        if (!points) {
          this.cancel()
          this.refuseFullCurve()
          return
        }
        gesture.points = points
        gesture.last = sample
        scene.setViewHold({
          automation: gesture.automation,
          range: gesture.range,
        })
        scene.setDraft({ automation: gesture.automation, points })
        this.showPointBadge(
          input,
          gesture.clip,
          points,
          points.findLastIndex((point) => point.tick === sample.tick)
        )
        break
      }
      case "point": {
        const dx = input.x - gesture.x
        const dy = input.y - gesture.y
        if (!gesture.dragging) {
          if (Math.hypot(dx, dy) < DRAG_THRESHOLD_PX) return
          gesture.dragging = true
          this.onCursor("move")
        }
        // In the view of the press, whatever the curve's own view would be
        // by now: the default one follows the points, this one included.
        const view = this.curveView(gesture.clip, gesture.range)
        const origin = gesture.base[gesture.index]
        if (!view || !origin) return
        const snap = this.snapFor(input)
        const songTick =
          snap > 0
            ? snapNearest(xToTick(this.surface.viewport, input.x), snap)
            : xToTick(this.surface.viewport, input.x)
        // Past the top or the bottom of the view the value goes on at the
        // same pace, and the view grows to show it.
        let target = {
          tick: toCurveTick(view.window, songTick),
          value: valueBeyond(view, input.y),
        }
        // Shift keeps the drag to the axis it has moved further along.
        if (input.shift) target = constrainToAxis(origin, target, dx, dy)
        gesture.points = gesture.added
          ? movePoint(
              gesture.base,
              gesture.index,
              target.tick,
              target.value,
              view.window
            )
          : moveSelectedPoints(
              gesture.base,
              gesture.indices,
              target.tick - origin.tick,
              target.value - origin.value,
              view.window
            )
        scene.setViewHold({
          automation: gesture.automation,
          range: extendView(
            gesture.range,
            ...[...gesture.indices].map((index) => gesture.points[index].value)
          ),
        })
        scene.setDraft({
          automation: gesture.automation,
          points: gesture.points,
        })
        this.showPointBadge(input, gesture.clip, gesture.points, gesture.index)
        return
      }
      case "bend": {
        if (!gesture.dragging) {
          if (Math.abs(input.y - gesture.y) < DRAG_THRESHOLD_PX) return
          gesture.dragging = true
        }
        const view = this.curveView(gesture.clip, gesture.range)
        const from = gesture.base[gesture.index]
        const to = gesture.base[gesture.index + 1]
        if (!view || !from || !to) return
        scene.setViewHold({
          automation: gesture.automation,
          range: gesture.range,
        })
        // The handle follows the pointer. Between two level points no bend
        // shows, so there the drag sets it by how far it has gone.
        const curve =
          bendThrough(from.value, to.value, valueAt(view, input.y)) ??
          clamp(from.curve + (gesture.y - input.y) / BEND_DRAG_PX, -1, 1)
        gesture.points = bendSegment(gesture.base, gesture.index, curve)
        scene.setDraft({
          automation: gesture.automation,
          points: gesture.points,
        })
        scene.setBadge({
          x: input.x,
          y: input.y,
          text:
            Math.abs(curve) < 0.005
              ? "Straight"
              : `Bend ${Math.round(curve * 100)}%`,
        })
        return
      }
      case "marquee": {
        const items = scene.items
        if (!items) return
        scene.setMarquee({
          tick0: gesture.from.tick,
          row0: gesture.from.row,
          tick1: point.tick,
          row1: point.row,
        })
        // Padded like a stroke, so a box dragged dead level still has area.
        const box = strokeBox(gesture.from, point)
        const ids = new Set(gesture.base)
        for (const index of queryRect(
          items,
          box.tick0,
          box.tick1,
          box.row0,
          box.row1
        )) {
          ids.add(items.batch.ids[index])
        }
        gesture.ids = new Set(
          expandClipSelection(
            ids,
            project().playlist.arrangementBook?.clipGroups ?? []
          )
        )
        // The store hears about it once, when the button comes up.
        scene.showSelected(gesture.ids)
        break
      }
      case "move": {
        if (!gesture.dragging) {
          const far =
            Math.hypot(input.x - gesture.x, input.y - gesture.y) >=
            DRAG_THRESHOLD_PX
          if (!far) return
          gesture.dragging = true
          this.onCursor("grabbing")
        }
        if (gesture.grouped) {
          const { tracks } = project().playlist
          const visualRows = Math.floor(point.row) - gesture.from.row
          const destination = documentRowFor(
            scene.rowOf(gesture.anchor.track) + visualRows
          )
          const anchorRow = tracks.findIndex(
            (track) => track.id === gesture.anchor.track
          )
          const ticks =
            snapTick(
              gesture.anchor.start + point.tick - gesture.from.tick,
              this.snapFor(input)
            ) - gesture.anchor.start
          gesture.updates =
            destination === undefined || anchorRow < 0
              ? null
              : clipGroupMove(
                  gesture.clips,
                  tracks,
                  ticks,
                  destination - anchorRow
                )
          scene.cloneHint = input.mod
          scene.setDrag(
            gesture.updates ? { ...NO_DRAG, ticks, rows: visualRows } : NO_DRAG
          )
          break
        }
        const move = clampMove(
          gesture.clips.map((clip) => ({
            start: clip.start,
            row: scene.rowOf(clip.track),
          })),
          gesture.anchor.start,
          point.tick - gesture.from.tick,
          Math.floor(point.row) - gesture.from.row,
          this.snapFor(input),
          this.surface.limits.rowCount
        )
        scene.cloneHint = input.mod
        scene.setDrag({ ...NO_DRAG, ticks: move.ticks, rows: move.rows })
        break
      }
      case "slip": {
        if (!gesture.dragging) {
          if (Math.abs(input.x - gesture.x) < DRAG_THRESHOLD_PX) return
          gesture.dragging = true
        }
        // Snap the signed travel, not the clip's position or its offset.
        // Dragging content right exposes earlier content in the fixed window.
        const delta = snapTick(
          point.tick - gesture.fromTick,
          this.snapFor(input)
        )
        gesture.offset = clamp(gesture.clip.offset - delta, 0, MAX_SONG_TICKS)
        scene.setSlipDraft({ clip: gesture.clip.id, offset: gesture.offset })
        scene.setBadge({
          x: input.x,
          y: input.y,
          text: `Offset: ${gesture.offset} ticks`,
        })
        break
      }
      case "resize": {
        if (!gesture.dragging) {
          if (Math.abs(input.x - gesture.x) < DRAG_THRESHOLD_PX) return
          gesture.dragging = true
        }
        const snap = this.snapFor(input)
        const minLength = minResizeLength(snap)
        scene.setDrag(
          gesture.edge === "end"
            ? {
                ...NO_DRAG,
                resizeEnd: endResizeDelta(gesture.anchor, point.tick, snap),
                minLength,
              }
            : {
                ...NO_DRAG,
                resizeStart: startTrimDelta(
                  gesture.clips,
                  gesture.anchor,
                  point.tick,
                  snap,
                  scene.passTicks
                ),
                minLength,
              }
        )
        break
      }
      case "place": {
        const start = snapCell(point.tick, this.snapFor(input))
        scene.setGhosts([
          brushClip(
            gesture.brush,
            start,
            this.rowAt(point),
            this.brushContext(gesture.brush, start)
          ),
        ])
        break
      }
      case "paint": {
        const pass = (tick: number) =>
          brushTicks(gesture.brush, this.brushContext(gesture.brush, tick))
        scene.setGhosts(
          withoutStacked(
            paintStarts(gesture.anchor, point.tick, pass).map((start) =>
              brushClip(
                gesture.brush,
                start,
                gesture.row,
                this.brushContext(gesture.brush, start)
              )
            ),
            scene.clips,
            scene.rowOf
          )
        )
        break
      }
      case "stroke": {
        const items = scene.items
        if (!items) return
        const box = strokeBox(gesture.last, point)
        gesture.last = point
        const marked = new Set(scene.marked)
        for (const index of queryRect(
          items,
          box.tick0,
          box.tick1,
          box.row0,
          box.row1
        )) {
          const clip = scene.clipById.get(items.batch.ids[index])
          if (!clip) continue
          gesture.muteTo ??= !clip.muted
          marked.add(clip.id)
        }
        const expanded =
          gesture.action === "erase"
            ? new Set(
                expandClipSelection(
                  marked,
                  project().playlist.arrangementBook?.clipGroups ?? []
                )
              )
            : marked
        if (expanded.size !== scene.marked.size) scene.setMarked(expanded)
        break
      }
      default: {
        const _exhaustive: never = gesture
        return _exhaustive
      }
    }
    this.edgeScroll(input)
  }

  /**
   * Scrolls while a drag is held at an edge of the grid, so a clip can be
   * taken to a bar or a track that is out of view. Each frame it scrolls a
   * little and moves the pointer again, which looks at the edge again.
   */
  private edgeScroll(input: PointerInput): void {
    // A press near an edge is not a drag toward it until it has moved.
    const from = this.pressedAt
    if (
      from &&
      Math.hypot(input.x - from.x, input.y - from.y) < DRAG_THRESHOLD_PX
    ) {
      return
    }
    this.pressedAt = null
    this.lastInput = input
    if (this.scrollFrame !== 0) return
    const { width, height } = this.surface.viewport
    if (edgePull(input.x, width) === 0 && edgePull(input.y, height) === 0) {
      return
    }
    this.scrollFrame = requestAnimationFrame(() => {
      this.scrollFrame = 0
      const held = this.lastInput
      const kind = this.gesture.kind
      if (!held || kind === "idle" || kind === "committing" || kind === "pan") {
        return
      }
      const before = this.surface.viewport
      this.metrics.panBy(
        edgePull(held.x, before.width),
        edgePull(held.y, before.height)
      )
      // At the end of the timeline or the rows there is nowhere to go.
      if (this.surface.viewport !== before) this.pointerMove(held)
    })
  }

  private stopEdgeScroll(): void {
    if (this.scrollFrame !== 0) cancelAnimationFrame(this.scrollFrame)
    this.scrollFrame = 0
    this.lastInput = null
  }

  /** Places what the Draw and Paint tools laid down. */
  private placeBrush(
    brush: ResolvedBrush,
    clips: readonly NewClip[],
    label: string
  ): Promise<ClipId[] | null> {
    if (brush.type !== "audio") return addClips(clips, label)
    const layout = trackRowsNow()
    const destinations = clips.map((clip) => documentRowFor(clip.row, layout))
    if (destinations.some((row) => row === undefined)) {
      return Promise.resolve(null)
    }
    return placeSampleClips(
      brush.sample,
      clips.map((clip, index) => ({ ...clip, row: destinations[index]! })),
      label
    )
  }

  /** The button came up. Resolves once the edit, if any, has been applied. */
  async pointerUp(input: PointerInput): Promise<void> {
    const gesture = this.gesture
    const scene = this.scene
    this.stopEdgeScroll()
    switch (gesture.kind) {
      case "idle":
      case "committing":
        return
      case "playback":
        this.settle()
        return
      case "slice": {
        const tick = this.sliceTick(this.pointAt(input).tick, input)
        const updates: ClipUpdate[] = []
        const clips: ClipInit[] = []
        // Use the document's clips directly: selection, visible rows and
        // clip groups do not change which spans strictly cross this line.
        for (const clip of project().playlist.clips) {
          const end = clip.start + clip.length
          if (tick <= clip.start || tick >= end) continue
          const leftLength = tick - clip.start
          updates.push({ id: clip.id, patch: { length: leftLength } })
          clips.push({
            track: clip.track,
            start: tick,
            length: end - tick,
            offset: clip.offset + leftLength,
            muted: clip.muted,
            content: clip.content,
          })
        }
        this.settle()
        if (clips.length === 0) return
        this.gesture = { kind: "committing" }
        await dispatch({
          type: "batch",
          label: plural(clips.length, t("Slice clip")),
          commands: [
            { type: "updateClips", updates },
            { type: "addClips", clips },
          ],
        })
        this.settle()
        return
      }
      case "pan":
        this.gesture = { kind: "idle" }
        this.pointerMove(input)
        return
      case "marquee":
        ui().select(gesture.ids)
        this.settle()
        return
      case "fade": {
        const before =
          gesture.edge === "in"
            ? gesture.content.fadeIn
            : gesture.content.fadeOut
        if (!gesture.dragging || gesture.ticks === before) {
          this.settle()
          return
        }
        this.gesture = { kind: "committing" }
        await changeAudioClips([
          {
            id: gesture.clip.id,
            patch:
              gesture.edge === "in"
                ? { fadeIn: gesture.ticks }
                : { fadeOut: gesture.ticks },
          },
        ])
        this.settle()
        return
      }
      case "gain": {
        if (!gesture.dragging || gesture.gain === gesture.content.gain) {
          this.settle()
          return
        }
        this.gesture = { kind: "committing" }
        await changeAudioClips([
          { id: gesture.clip.id, patch: { gain: gesture.gain } },
        ])
        this.settle()
        return
      }
      case "step": {
        // Include the release position even if no move event arrived for it.
        this.pointerMove(input)
        this.stopEdgeScroll()
        if (this.gesture.kind !== "step") return
        this.gesture = { kind: "committing" }
        const generation = getProjectGeneration()
        await setCurve(gesture.automation, gesture.points)
        if (generation !== getProjectGeneration()) return
        this.settle()
        return
      }
      case "point":
      case "bend": {
        const added = gesture.kind === "point" && gesture.added
        if (!gesture.dragging && !added) {
          if (gesture.kind === "point" && gesture.selectOnlyOnClick) {
            scene.setPointSelection({
              clip: gesture.clip.id,
              automation: gesture.automation,
              indices: new Set([gesture.index]),
            })
          }
          this.settle()
          return
        }
        this.gesture = { kind: "committing" }
        // A point that left the chosen view takes the view's edge along.
        if (gesture.kind === "point") {
          showInView(
            gesture.automation,
            [...gesture.indices].map((index) => gesture.points[index].value)
          )
        }
        const generation = getProjectGeneration()
        const done = await setCurve(gesture.automation, gesture.points)
        if (generation !== getProjectGeneration()) return
        if (done && gesture.kind === "point" && added) {
          scene.setPointSelection({
            clip: gesture.clip.id,
            automation: gesture.automation,
            indices: gesture.indices,
          })
        }
        this.settle()
        return
      }
      case "move": {
        if (!gesture.dragging) {
          const id = gesture.anchor.id
          if (gesture.grouped) {
            ui().select(
              expandClipSelection(
                gesture.additive ? ui().selection : [id],
                project().playlist.arrangementBook?.clipGroups ?? []
              )
            )
          } else if (gesture.additive && gesture.wasSelected) {
            ui().select([...ui().selection].filter((other) => other !== id))
          } else if (!gesture.additive) {
            ui().select([id])
          }
          this.settle()
          return
        }
        const { ticks, rows } = scene.drag
        if (
          (gesture.grouped && gesture.updates === null) ||
          (ticks === 0 && rows === 0)
        ) {
          this.settle()
          return
        }
        this.gesture = { kind: "committing" }
        // Ctrl at the drop copies. Shift only ever adds to the selection.
        if (input.mod) {
          // The originals stay, so they go back and the copies show as
          // clips to come until they arrive.
          const copies = cloneMoved(gesture.clips, scene.rowOf, ticks, rows)
          scene.cloneHint = false
          scene.setDrag(NO_DRAG)
          scene.setGhosts(copies)
          const created = await addClips(
            copies,
            plural(copies.length, "Clone clip")
          )
          if (created && created.length > 0) ui().select(created)
        } else if (gesture.grouped) {
          await dispatch({
            type: "batch",
            label: plural(gesture.clips.length, t("Move clip")),
            commands: [{ type: "updateClips", updates: gesture.updates! }],
          })
        } else {
          await changeClips(
            moveChanges(gesture.clips, scene.rowOf, ticks, rows),
            plural(gesture.clips.length, "Move clip")
          )
        }
        this.settle()
        return
      }
      case "slip": {
        if (!gesture.dragging || gesture.offset === gesture.clip.offset) {
          this.settle()
          return
        }
        this.gesture = { kind: "committing" }
        await dispatch({
          type: "updateClips",
          updates: [{ id: gesture.clip.id, patch: { offset: gesture.offset } }],
        })
        this.settle()
        return
      }
      case "resize": {
        const changes = gesture.dragging
          ? resizeChanges(
              gesture.clips,
              scene.drag.resizeStart,
              scene.drag.resizeEnd,
              scene.drag.minLength,
              scene.passTicks
            )
          : []
        if (changes.length === 0) {
          this.settle()
          return
        }
        this.gesture = { kind: "committing" }
        await changeClips(
          changes,
          plural(
            changes.length,
            gesture.edge === "end" ? "Resize clip" : "Trim clip"
          )
        )
        this.settle()
        return
      }
      case "place":
      case "paint": {
        const clips = scene.ghosts
        this.gesture = { kind: "committing" }
        await this.placeBrush(
          gesture.brush,
          clips,
          gesture.kind === "place"
            ? "Add clip"
            : plural(clips.length, "Paint clip")
        )
        this.settle()
        return
      }
      case "stroke": {
        const ids = [...scene.marked]
        if (ids.length === 0) {
          this.settle()
          return
        }
        this.gesture = { kind: "committing" }
        if (gesture.action === "erase") await deleteClips(ids)
        else await setClipsMuted(ids, gesture.muteTo ?? true)
        this.settle()
        return
      }
      default: {
        const _exhaustive: never = gesture
        return _exhaustive
      }
    }
  }

  /** Drops a drag without doing anything: Escape, or a lost pointer. */
  cancel(): void {
    if (this.gesture.kind === "committing") return
    if (this.gesture.kind === "marquee") this.scene.showSelection()
    this.settle()
  }

  /** The pointer left the grid. */
  pointerLeave(): void {
    if (this.gesture.kind !== "idle") return
    this.scene.setHover(null)
    this.showInner(null)
  }

  private showInner(part: InnerHit["kind"] | null): void {
    if (part === this.innerShown) return
    this.innerShown = part
    this.onInner(part)
  }

  /**
   * Shows where a file dragged in from the browser would land: on the row
   * under the pointer, at the grid line of the cell it is in. `ticks` is
   * the clip's length, once the file's length is known. Null hides it.
   */
  previewDrop(
    input: { x: number; y: number; alt: boolean } | null,
    name = "",
    ticks: number | null = null
  ): { row: number; start: number } | null {
    if (!input) {
      this.scene.setDropPreview(null)
      return null
    }
    const point = this.pointAt(input)
    const row = this.rowAt(point)
    const start = snapCell(point.tick, this.snapFor(input))
    const bar = ticksPerBar(project().settings.timeSignature)
    this.scene.setDropPreview({ row, start, length: ticks ?? bar, name })
    return { row, start }
  }
}
