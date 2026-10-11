import type {
  AutomationId,
  AutomationPoint,
  Clip,
  ClipId,
  PlaylistTrack,
  PlaylistTrackId,
  Project,
  SampleId,
} from "@/bindings"
import {
  compileLanes,
  holdSegments,
  type HoldSegment,
} from "@/lib/automation/lanes"
import {
  FULL_VIEW,
  viewOfAutomation,
  type ViewRange,
} from "@/lib/automation/view-range"
import {
  indexBatch,
  queryRect,
  xToTick,
  type Hit,
  type IndexedBatch,
  type Marquee,
  type RowStyle,
} from "@/lib/canvas"
import { useProjectStore } from "@/lib/store/project"
import { ticksPerBar } from "@/lib/time"

import { onPeaksChanged, samplePeaks, type SamplePeaks } from "./audio/peaks"
import { arrangementPlaylist } from "./arrangement-view"
import { buildClipBatch } from "./clip-batch"
import { patternTicks, songEnd, type NewClip } from "./edit"
import { formatAutomationValue } from "./automation/format"
import { POINT_RADIUS } from "./automation/hit"
import { useCurveViews } from "./automation/view-store"
import { innerHit, type InnerHit } from "./inner"
import { contentTicksFor, EDGE_PX, rowCountFor } from "./layout"
import {
  clipPattern,
  colorSignature,
  lookupsOf,
  NO_LOOKUPS,
  type ClipLookups,
} from "./look"
import type { GridMetrics } from "./metrics"
import {
  ClipPainter,
  NO_DRAG,
  type AudioDraft,
  type Badge,
  type CurveDraft,
  type DragState,
  type DropPreview,
  type PaintSource,
  type PointSelection,
  type SlipDraft,
} from "./painter"
import { playlistYToRow, rowGeometry } from "./row-geometry"
import { project } from "./selectors"
import { gridSpecFor } from "./snap"
import { usePlaylistStore } from "./store"
import type { GridSurface } from "./surface"
import { buildTrackRows, type TrackRow } from "./track-rows"

/** A point or a bend handle of a curve, and the clip that shows it. */
export type CurveMark = {
  clip: Clip
  inner: Extract<InnerHit, { kind: "point" | "bend" }>
}

const ui = () => usePlaylistStore.getState()
const NOTHING: ReadonlySet<ClipId> = new Set()
const NO_HOLDS: readonly HoldSegment[] = []
const NOT_SHARED: ReadonlySet<AutomationId> = new Set()

/**
 * What the grid shows: the clips of the project as a batch on the canvas,
 * the rows and grid lines that fit them, and the preview of a gesture in
 * progress. It follows the stores by itself and rebuilds the batch once per
 * edit. A change of selection only sets flags on the batch.
 */
export class PlaylistScene implements PaintSource {
  readonly painter: ClipPainter
  /** Called when an edit changed the clips, before the canvas shows them. */
  onClipsChanged: () => void = () => {}

  items: IndexedBatch | null = null
  clips: readonly Clip[] = []
  clipById: ReadonlyMap<ClipId, Clip> = new Map()
  lookups: ClipLookups = NO_LOOKUPS
  drag: DragState = NO_DRAG
  ghosts: readonly NewClip[] = []
  marked: ReadonlySet<ClipId> = NOTHING
  cloneHint = false
  songEnd = 0
  tempoBpm = 120
  draft: CurveDraft | null = null
  pointSelection: PointSelection | null = null
  audioDraft: AudioDraft | null = null
  slipDraft: SlipDraft | null = null
  holds: readonly HoldSegment[] = NO_HOLDS
  shared: ReadonlySet<AutomationId> = NOT_SHARED
  hover: ClipId | null = null
  badge: Badge | null = null
  dropPreview: DropPreview | null = null
  /**
   * The view a curve is held in while one of its points is dragged. The
   * default view follows the points, and would slide under the pointer
   * that is moving one of them.
   */
  viewHold: { automation: AutomationId; range: ViewRange } | null = null

  private readonly surface: GridSurface
  private readonly metrics: GridMetrics
  private readonly stops: (() => void)[]
  private tracks: readonly PlaylistTrack[] = []
  private visibleRows: readonly TrackRow[] = []
  private collapsedGroups: ReadonlySet<number> | null = null
  private seen: Project | null = null
  private colors = ""
  private rows = new Map<PlaylistTrackId, number>()
  private mutedTracks = new Set<PlaylistTrackId>()
  private rowCount = 0

  constructor(surface: GridSurface, metrics: GridMetrics) {
    this.surface = surface
    this.metrics = metrics
    this.painter = new ClipPainter(this)
    this.stops = [
      surface.addOverlayPainter(this.painter.paint),
      surface.onThemeChange(() => this.rebuild()),
      surface.onViewportChange(() => this.fitRows()),
      useProjectStore.subscribe((state, previous) => {
        const now = state.project
        const before = previous.project
        if (
          now.playlist !== before.playlist ||
          now.patterns !== before.patterns ||
          now.samples !== before.samples ||
          now.mixer !== before.mixer ||
          now.automations !== before.automations ||
          now.settings.timeSignature !== before.settings.timeSignature ||
          now.settings.tempoBpm !== before.settings.tempoBpm
        ) {
          this.sync()
        }
      }),
      usePlaylistStore.subscribe((state, previous) => {
        if (state.collapsedGroups !== previous.collapsedGroups) this.sync()
        if (state.trackHeights !== previous.trackHeights) {
          this.metrics.setViewport(this.surface.viewport)
          this.redraw()
        }
        if (state.selection !== previous.selection) this.showSelection()
        if (state.snap !== previous.snap) this.showGrid()
      }),
      // A waveform that has been read is drawn into its clips.
      onPeaksChanged(() => this.redraw()),
      useCurveViews.subscribe(() => this.redraw()),
    ]
    this.sync()
  }

  destroy(): void {
    for (const stop of this.stops) stop()
  }

  isMuted = (clip: Clip): boolean =>
    clip.muted || this.mutedTracks.has(clip.track)

  rowOf = (track: PlaylistTrackId): number => this.rows.get(track) ?? 0

  trackRow = (track: PlaylistTrackId): number | undefined =>
    this.rows.get(track)

  peaksOf = (sample: SampleId): SamplePeaks | null => {
    const asset = this.lookups.samples.get(sample)
    return asset ? samplePeaks(asset) : null
  }

  /** The curve an automation clip shows, as it is being edited if it is. */
  pointsOf(automation: AutomationId): readonly AutomationPoint[] | null {
    if (this.draft?.automation === automation) return this.draft.points
    return this.lookups.automations.get(automation)?.points ?? null
  }

  /**
   * The part of its range an automation's clips show, bottom to top: the
   * one a drag is holding, the one chosen for it, or its default.
   */
  viewOf = (automation: AutomationId): ViewRange => {
    if (this.viewHold?.automation === automation) return this.viewHold.range
    const found = this.lookups.automations.get(automation)
    if (!found) return FULL_VIEW
    return viewOfAutomation(
      found,
      this.tempoBpm,
      useCurveViews.getState().views[automation]
    )
  }

  /** The two ends of an automation's view in the unit of what it moves. */
  viewLabelsOf = (
    automation: AutomationId
  ): { top: string; bottom: string } | null => {
    const found = this.lookups.automations.get(automation)
    if (!found) return null
    const view = this.viewOf(automation)
    const current = project()
    return {
      top: formatAutomationValue(current, found.target, view.hi),
      bottom: formatAutomationValue(current, found.target, view.lo),
    }
  }

  setViewHold(hold: { automation: AutomationId; range: ViewRange } | null) {
    this.viewHold = hold
  }

  /**
   * The part of a clip under a point, in CSS pixels, that has an edit of
   * its own: a fade or gain handle, a point of a curve.
   */
  innerAt(clip: Clip, x: number, y: number): InnerHit | null {
    const automation =
      clip.content.type === "automation" ? clip.content.automation : null
    return innerHit(
      this.surface.viewport,
      clip,
      this.rowOf(clip.track),
      automation === null ? null : this.pointsOf(automation),
      x,
      y,
      automation === null ? undefined : this.viewOf(automation)
    )
  }

  /**
   * One pass of the pattern a clip plays, in ticks. 0 for a clip of audio
   * or automation, which does not loop.
   */
  passTicks = (clip: Clip): number => {
    if (clip.content.type !== "pattern") return 0
    const pattern = clipPattern(clip, this.lookups)
    return pattern ? patternTicks(pattern) : clip.length
  }

  /** The clip under a point given in CSS pixels. */
  hitAt(x: number, y: number): Hit | null {
    return this.surface.hitTest(x, y, { edgePx: EDGE_PX })
  }

  /**
   * A point or a bend handle of a curve under a spot of the grid, whatever
   * clip or empty grid the spot itself is on. These are looked for first:
   * the dot of a point on a clip's edge hangs over the edge, and a press on
   * that half of it is a press on the point, not on what lies beside the
   * clip. A point wins over a handle.
   */
  curveMarkAt(x: number, y: number): CurveMark | null {
    const items = this.items
    if (!items) return null
    const viewport = this.surface.viewport
    const tick = xToTick(viewport, x)
    const ticks = POINT_RADIUS / viewport.pxPerTick
    let handle: CurveMark | null = null
    for (const index of queryRect(
      items,
      tick - ticks,
      tick + ticks,
      playlistYToRow(viewport, y - POINT_RADIUS),
      playlistYToRow(viewport, y + POINT_RADIUS)
    )) {
      const clip = this.clipById.get(items.batch.ids[index])
      if (clip?.content.type !== "automation") continue
      const inner = this.innerAt(clip, x, y)
      if (inner?.kind === "point") return { clip, inner }
      if (inner?.kind === "bend") handle ??= { clip, inner }
    }
    return handle
  }

  /** Reads the project again and redraws. Runs after every edit. */
  private sync(): void {
    const current = project()
    const seen = this.seen
    this.seen = current
    const { playlist } = current
    const clipsChanged = !seen || playlist.clips !== seen.playlist.clips
    const tracksChanged = playlist.tracks !== this.tracks
    const rowsChanged =
      tracksChanged ||
      playlist.arrangementBook !== seen?.playlist.arrangementBook ||
      ui().collapsedGroups !== this.collapsedGroups
    const lookupsChanged =
      !seen ||
      seen.patterns !== current.patterns ||
      seen.samples !== current.samples ||
      seen.mixer !== current.mixer ||
      seen.automations !== current.automations
    if (lookupsChanged) this.lookups = lookupsOf(current)
    const selectedPoints = this.pointSelection
    if (selectedPoints) {
      const clip = playlist.clips.find(
        (clip) => clip.id === selectedPoints.clip
      )
      const points = this.lookups.automations.get(
        selectedPoints.automation
      )?.points
      const before = seen?.automations.find(
        (automation) => automation.id === selectedPoints.automation
      )?.points
      if (
        clip?.content.type !== "automation" ||
        clip.content.automation !== selectedPoints.automation ||
        !points ||
        (before && before.length !== points.length)
      ) {
        // Inserting or deleting a point invalidates indices held by the view.
        this.pointSelection = null
      }
    }
    const colors = lookupsChanged ? colorSignature(this.lookups) : this.colors
    const colorsChanged = colors !== this.colors
    this.colors = colors
    // A fader move changes the mixer and nothing that is drawn here.
    const onlyMixer =
      seen !== null &&
      !clipsChanged &&
      !tracksChanged &&
      !rowsChanged &&
      !colorsChanged &&
      seen.patterns === current.patterns &&
      seen.samples === current.samples &&
      seen.automations === current.automations &&
      seen.playlist.timeline?.meters === current.playlist.timeline?.meters &&
      seen.settings === current.settings
    if (onlyMixer) return

    this.tracks = playlist.tracks
    const activePlaylist = arrangementPlaylist(playlist)
    if (rowsChanged) {
      const layout = buildTrackRows(playlist, ui().collapsedGroups)
      this.visibleRows = layout.rows
      this.rows = layout.trackRows
      this.collapsedGroups = ui().collapsedGroups
    }
    if (tracksChanged) {
      this.mutedTracks = new Set(
        playlist.tracks.filter((track) => track.muted).map((track) => track.id)
      )
    }
    if (clipsChanged || rowsChanged) {
      this.clips = activePlaylist.clips.filter((clip) =>
        this.rows.has(clip.track)
      )
      this.clipById = new Map(
        activePlaylist.clips.map((clip) => [clip.id, clip])
      )
      this.onClipsChanged()
    }
    if (clipsChanged || rowsChanged) {
      this.songEnd = songEnd(activePlaylist.clips)
    }
    this.tempoBpm = current.settings.tempoBpm
    if (
      clipsChanged ||
      rowsChanged ||
      tracksChanged ||
      !seen ||
      seen.automations !== current.automations
    ) {
      this.followAutomation({ ...current, playlist: activePlaylist })
    }

    const { selection } = ui()
    if ([...selection].some((id) => !this.clipById.has(id))) {
      // Clips can go away under the selection: undo, or a deleted pattern.
      ui().select([...selection].filter((id) => this.clipById.has(id)))
    }
    this.showGrid()
    this.fitRows(true)
    // The batch holds places and colors. What is inside a clip (its notes,
    // its curve, its waveform) is painted over it and needs no new batch.
    if (clipsChanged || rowsChanged || colorsChanged) this.rebuild()
    else this.redraw()
  }

  /**
   * Works out what the automation clips leave behind them: where each
   * target holds a clip's end value, and which curves several clips show.
   */
  private followAutomation(current: Project): void {
    if (current.automations.length === 0 && !current.patterns.some((pattern) => pattern.events?.length)) {
      this.holds = NO_HOLDS
      this.shared = NOT_SHARED
      return
    }
    this.holds = holdSegments(compileLanes(current), this.songEnd)
    const seen = new Set<AutomationId>()
    const shared = new Set<AutomationId>()
    for (const clip of current.playlist.clips) {
      if (clip.content.type !== "automation") continue
      const id = clip.content.automation
      if (seen.has(id)) shared.add(id)
      seen.add(id)
    }
    this.shared = shared
  }

  private rebuild(): void {
    this.items = indexBatch(
      buildClipBatch(
        this.clips,
        this.rowOf,
        this.lookups,
        this.isMuted,
        ui().selection,
        this.surface.theme
      )
    )
    this.surface.setItems(this.items)
    this.painter.invalidate()
  }

  /** Marks these clips selected on the canvas. The store is not told. */
  showSelected(ids: Iterable<ClipId>): void {
    const items = this.items
    if (!items) return
    const indices: number[] = []
    for (const id of ids) {
      const index = items.batch.indexOfId(id)
      if (index >= 0) indices.push(index)
    }
    items.batch.setSelection(indices)
    this.redraw()
  }

  /** Shows the selection the store holds. */
  showSelection(): void {
    this.showSelected(ui().selection)
  }

  private showGrid(): void {
    const current = project()
    this.surface.setTimeGrid(
      gridSpecFor(
        ui().snap,
        current.settings.timeSignature,
        current.playlist.timeline?.meters
      )
    )
  }

  /**
   * Sets how many rows and how much timeline the grid has. Rows below the
   * last track and the rows of muted tracks are the darker ones.
   */
  private fitRows(force = false): void {
    const viewport = this.surface.viewport
    const rowCount = rowCountFor(
      this.visibleRows.length,
      rowGeometry(viewport).rowAt(viewport.height)
    )
    const contentTicks = contentTicksFor(
      this.songEnd,
      ticksPerBar(project().settings.timeSignature)
    )
    const limits = this.surface.limits
    if (
      !force &&
      rowCount === this.rowCount &&
      contentTicks === limits.contentTicks
    ) {
      return
    }
    this.rowCount = rowCount
    const shaded = new Uint8Array(rowCount)
    const strong = new Uint8Array(rowCount + 1)
    this.visibleRows.forEach((entry, row) => {
      if (entry.kind === "group") {
        shaded[row] = 1
        strong[row] = 1
      } else if (entry.track.muted) shaded[row] = 1
    })
    for (let row = this.visibleRows.length; row < rowCount; row++)
      shaded[row] = 1
    if (this.visibleRows.length > 0) strong[this.visibleRows.length] = 1
    const style: RowStyle = { rowCount, shaded, strong }
    this.surface.setRows(style)
    if (
      force ||
      rowCount !== limits.rowCount ||
      contentTicks !== limits.contentTicks
    ) {
      this.metrics.setLimits({ rowCount, contentTicks })
    }
  }

  // The preview of a gesture. None of it touches the batch's geometry.

  redraw(): void {
    this.painter.invalidate()
    this.surface.invalidate("all")
  }

  /** Shows the selected clips moved or resized by this much. */
  setDrag(drag: DragState): void {
    this.drag = drag
    this.surface.setDragOffset(drag.ticks, drag.rows)
    this.surface.setDragResize(drag.resizeStart, drag.resizeEnd, drag.minLength)
    this.redraw()
  }

  setGhosts(ghosts: readonly NewClip[]): void {
    this.ghosts = ghosts
    this.redraw()
  }

  setMarked(marked: ReadonlySet<ClipId>): void {
    this.marked = marked
    this.redraw()
  }

  setMarquee(marquee: Marquee | null): void {
    this.surface.setMarquee(marquee)
  }

  /** Shows a curve as it is being edited, in every clip of its automation. */
  setDraft(draft: CurveDraft | null): void {
    this.draft = draft
    this.redraw()
  }

  setPointSelection(selection: PointSelection | null): void {
    this.pointSelection = selection
    this.redraw()
  }

  /** Shows an audio clip with settings a handle is being dragged to. */
  setAudioDraft(draft: AudioDraft | null): void {
    this.audioDraft = draft
    this.redraw()
  }

  /** Slides content in the painter without changing the clip's geometry. */
  setSlipDraft(draft: SlipDraft | null): void {
    this.slipDraft = draft
    this.redraw()
  }

  setBadge(badge: Badge | null): void {
    if (badge === null && this.badge === null) return
    this.badge = badge
    this.redraw()
  }

  /** Marks the clip under the pointer, which shows its handles. */
  setHover(clip: ClipId | null): void {
    if (clip === this.hover) return
    this.hover = clip
    this.redraw()
  }

  setDropPreview(preview: DropPreview | null): void {
    const current = this.dropPreview
    if (
      preview?.row === current?.row &&
      preview?.start === current?.start &&
      preview?.length === current?.length &&
      preview?.name === current?.name
    ) {
      return
    }
    this.dropPreview = preview
    this.redraw()
  }

  clearPreview(): void {
    this.ghosts = []
    this.marked = NOTHING
    this.cloneHint = false
    this.draft = null
    this.viewHold = null
    this.audioDraft = null
    this.slipDraft = null
    this.badge = null
    this.surface.setMarquee(null)
    this.setDrag(NO_DRAG)
  }
}
