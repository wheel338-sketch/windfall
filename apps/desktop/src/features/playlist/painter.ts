import type {
  AutomationId,
  AutomationPoint,
  Clip,
  ClipContent,
  ClipId,
  PlaylistTrackId,
  SampleId,
} from "@/bindings"
import type { HoldSegment } from "@/lib/automation/lanes"
import type { ViewRange } from "@/lib/automation/view-range"
import {
  BORDER_SHADE,
  deviceX,
  mix,
  resizedSpan,
  rgbaToCss,
  rgbFromInt,
  visibleRange,
  visibleTicks,
  withAlpha,
  type GridTheme,
  type IndexedBatch,
  type OverlayFrame,
  type OverlayPainter,
  type Rgba,
} from "@/lib/canvas"

import {
  GAIN_HANDLE_INSET,
  GAIN_HANDLE_WIDTH,
  MIN_WIDTH_FOR_GAIN,
} from "./audio/handles"
import { paintAudio, paintMissing } from "./audio/paint"
import type { SamplePeaks } from "./audio/peaks"
import { paintAutomation, paintHolds, paintLinkGlyph } from "./automation/paint"
import { clipBodyColor } from "./clip-batch"
import {
  BAND_HEIGHT,
  CONTENT_PAD,
  MIN_HEIGHT_FOR_BAND,
  type Box,
} from "./clip-box"
import { patternTicks, type NewClip } from "./edit"
import {
  contentColor,
  contentName,
  ORPHAN_COLOR,
  type ClipLookups,
} from "./look"
import { playlistDeviceY, playlistYToRow } from "./row-geometry"
import { loopPoints, previewOf, previewSpans, type ClipSpan } from "./preview"
import type { ClipSprite, ClipStyle } from "./sprite"
import { translatedText as t, message } from "@/lib/i18n"

/** How a drag in progress displaces the selected clips. */
export type DragState = {
  ticks: number
  rows: number
  resizeStart: number
  resizeEnd: number
  minLength: number
}

export const NO_DRAG: DragState = {
  ticks: 0,
  rows: 0,
  resizeStart: 0,
  resizeEnd: 0,
  minLength: 0,
}

/** A value shown beside the pointer while something is dragged. */
export type Badge = {
  /** CSS pixels from the grid's top left corner. */
  x: number
  y: number
  text: string
}

/** Where a file dragged in from the browser would land. */
export type DropPreview = {
  row: number
  start: number
  length: number
  name: string
}

/** A curve as it is being edited, before the edit has gone to the project. */
export type CurveDraft = {
  automation: AutomationId
  points: readonly AutomationPoint[]
}

/** Session-only point indices, belonging to one clip even for shared curves. */
export type PointSelection = {
  clip: ClipId
  automation: AutomationId
  indices: ReadonlySet<number>
}

export type AudioContent = Extract<ClipContent, { type: "audio" }>

/** An audio clip's settings as a drag has them, before it is released. */
export type AudioDraft = { clip: ClipId; content: AudioContent }

/** A clip's content offset while it is being slipped, before release. */
export type SlipDraft = { clip: ClipId; offset: number }

/** What the painter draws from. Whoever owns it calls `invalidate` after a change. */
export interface PaintSource {
  readonly items: IndexedBatch | null
  readonly clipById: ReadonlyMap<ClipId, Clip>
  readonly lookups: ClipLookups
  isMuted(clip: Clip): boolean
  /** The row of a playlist track, or undefined for a track that is gone. */
  trackRow(track: PlaylistTrackId): number | undefined
  /** The waveform of a sample, or null when the project has no such sample. */
  peaksOf(sample: SampleId): SamplePeaks | null
  readonly tempoBpm: number
  readonly drag: DragState
  /** Clips that will exist once the button is released. */
  readonly ghosts: readonly NewClip[]
  /** Clips an erase or mute stroke has crossed. */
  readonly marked: ReadonlySet<ClipId>
  /** Show where the dragged clips came from, because they will stay there. */
  readonly cloneHint: boolean
  readonly songEnd: number
  /** The curve being edited, shown in every clip of its automation. */
  readonly draft: CurveDraft | null
  readonly pointSelection?: PointSelection | null
  readonly audioDraft: AudioDraft | null
  readonly slipDraft: SlipDraft | null
  /** Where targets stay on the value an automation clip left. */
  readonly holds: readonly HoldSegment[]
  /** Automations more than one clip shows. */
  readonly shared: ReadonlySet<AutomationId>
  /** The part of its range an automation's clips show, bottom to top. */
  viewOf(automation: AutomationId): ViewRange
  /** The two ends of that view in the unit of what the automation moves. */
  viewLabelsOf(automation: AutomationId): { top: string; bottom: string } | null
  /** The clip under the pointer. */
  readonly hover: ClipId | null
  readonly badge: Badge | null
  readonly dropPreview: DropPreview | null
}

export type PaintStats = {
  /** Clips that got a name, a preview or both. */
  clips: number
  /** Note rects drawn for the previews. */
  notes: number
  /** Columns of waveform drawn for the audio clips. */
  columns: number
  /** Main-thread time of the last full repaint, in milliseconds. */
  ms: number
  /** Full repaints so far. A frame that only moves the playhead is not one. */
  repaints: number
}

// Sizes in CSS pixels. A clip smaller than these goes without that part.
const MIN_CLIP_WIDTH = 5
const MIN_LABEL_WIDTH = 22
const MIN_LABEL_HEIGHT = 12
const MIN_PREVIEW_HEIGHT = 6
const MIN_PASS_WIDTH = 10
const MAX_NOTE_HEIGHT = 4
const TEXT_PAD = 4
const FONT_SIZE = 10.5
const FIT_BUCKET = 4
/** An automation clip narrower than this has no room for the link glyph. */
const MIN_LINK_WIDTH = 48

/** Pixels of width a pass of the pattern needs per note before it is drawn. */
const NOTE_SPACING = 1.5
/**
 * Note rects per repaint. The spacing rule keeps a full screen well under
 * this; past it the remaining clips go without a preview.
 */
const NOTE_BUDGET = 20000
const DIVIDER_BUDGET = 1500
/** Columns of waveform per repaint: about thirty screens' width. */
const COLUMN_BUDGET = 60000
/** Points and bend handles of automation curves per repaint. */
const MARK_BUDGET = 6000

const DARK_INK = "rgba(0,0,0,0.84)"
const LIGHT_INK = "rgba(255,255,255,0.96)"

function luminance(color: Rgba): number {
  return 0.2126 * color.r + 0.7152 * color.g + 0.0722 * color.b
}

function inkOn(color: Rgba): string {
  return luminance(color) > 150 ? DARK_INK : LIGHT_INK
}

/** A CSS variable of the theme, for the few colors the grid's theme lacks. */
function themeColor(name: string, fallback: string): string {
  if (typeof document === "undefined") return fallback
  const value = getComputedStyle(document.documentElement)
    .getPropertyValue(name)
    .trim()
  return value === "" ? fallback : value
}

/**
 * Draws what the rect renderer cannot: each clip's title bar and name, and
 * what is inside it: a pattern's notes, an audio clip's waveform and fades,
 * an automation clip's curve. Also the loop dividers, the previews of a
 * gesture in progress and the lines that show a held automation value.
 *
 * The grid's overlay is repainted for every playhead move, 60 times a second
 * while the song plays. None of this changes then, so it is painted into a
 * canvas of its own and that is copied into the overlay. It is repainted
 * only when the view, the clips or a gesture change.
 */
export class ClipPainter {
  readonly stats: PaintStats = {
    clips: 0,
    notes: 0,
    columns: 0,
    ms: 0,
    repaints: 0,
  }

  private readonly source: PaintSource
  private layer: HTMLCanvasElement | null = null
  private layerContext: CanvasRenderingContext2D | null = null
  private stale = true
  private key = ""
  private theme: GridTheme | null = null
  private styles = new Map<number, ClipStyle>()
  private warn = ""
  private font = ""
  private widths = new Map<string, number>()
  private fits = new Map<string, string>()

  constructor(source: PaintSource) {
    this.source = source
  }

  /** Call after anything in the source changed. */
  invalidate(): void {
    this.stale = true
  }

  readonly paint: OverlayPainter = (ctx, frame) => {
    const { viewport, transform } = frame
    const key = `${viewport.scrollTick}|${viewport.scrollRow}|${viewport.pxPerTick}|${viewport.rowHeight}|${transform.widthDev}|${transform.heightDev}|${viewport.dpr}`
    const layer = this.layerFor(transform.widthDev, transform.heightDev)
    if (!layer || !this.layerContext) return
    if (this.stale || key !== this.key || frame.theme !== this.theme) {
      const started = performance.now()
      this.repaint(this.layerContext, frame)
      this.stats.ms = performance.now() - started
      this.stats.repaints++
      this.stale = false
      this.key = key
    }
    ctx.drawImage(layer, 0, 0)
  }

  private layerFor(width: number, height: number): HTMLCanvasElement | null {
    if (!this.layer) {
      this.layer = document.createElement("canvas")
      this.layerContext = this.layer.getContext("2d")
    }
    if (this.layer.width !== width || this.layer.height !== height) {
      this.layer.width = width
      this.layer.height = height
      this.stale = true
    }
    return this.layer
  }

  private styleFor(
    theme: GridTheme,
    color: number,
    selected: boolean,
    muted: boolean
  ): number {
    const key = color * 4 + (selected ? 1 : 0) + (muted ? 2 : 0)
    if (this.styles.has(key)) return key
    const select = (fill: Rgba) =>
      selected ? mix(fill, theme.selectionFill, theme.selectionMix) : fill
    const body = select(clipBodyColor(theme, color, muted))
    const band = select(
      muted ? mix(theme.background, rgbFromInt(color), 0.4) : rgbFromInt(color)
    )
    const border = selected
      ? theme.selectionBorder
      : {
          r: Math.round(body.r * BORDER_SHADE),
          g: Math.round(body.g * BORDER_SHADE),
          b: Math.round(body.b * BORDER_SHADE),
          a: body.a,
        }
    const quiet = rgbaToCss(theme.mutedForeground)
    this.styles.set(key, {
      body: rgbaToCss(body),
      border: rgbaToCss(border),
      band: rgbaToCss(band),
      bandInk: muted ? quiet : inkOn(band),
      bodyInk: muted ? quiet : inkOn(body),
      note: rgbaToCss(mix(body, theme.foreground, muted ? 0.3 : 0.82)),
    })
    return key
  }

  /** A clip's content, as a drag of one of its handles has it. */
  private contentOf(clip: Clip): ClipContent {
    const draft = this.source.audioDraft
    return draft && draft.clip === clip.id ? draft.content : clip.content
  }

  private collect(frame: OverlayFrame): ClipSprite[] {
    const { viewport, transform, theme } = frame
    const { items, clipById, lookups, drag } = this.source
    const lw = transform.lineWidth
    const minWidth = MIN_CLIP_WIDTH * viewport.dpr
    const ticks = visibleTicks(viewport)
    const firstRow = Math.floor(playlistYToRow(viewport, 0))
    const lastRow = Math.ceil(playlistYToRow(viewport, viewport.height))
    const sprites: ClipSprite[] = []

    const add = (
      id: ClipId,
      span: ClipSpan,
      row: number,
      content: ClipContent,
      selected: boolean,
      muted: boolean,
      ghost: boolean
    ) => {
      if (row < firstRow || row >= lastRow) return
      if (span.start >= ticks.end || span.start + span.length <= ticks.start) {
        return
      }
      const x0 = deviceX(transform, span.start)
      const x1 = Math.max(x0 + lw, deviceX(transform, span.start + span.length))
      if (x1 - x0 < minWidth && !ghost && !this.source.marked.has(id)) return
      sprites.push({
        id,
        style: this.styleFor(
          theme,
          contentColor(content, lookups),
          selected,
          muted
        ),
        content,
        name: contentName(content, lookups),
        pattern:
          content.type === "pattern"
            ? lookups.patterns.get(content.pattern)
            : undefined,
        span,
        x0,
        x1,
        y0: playlistDeviceY(viewport, transform, row) + lw,
        y1: playlistDeviceY(viewport, transform, row + 1),
        ghost,
        selected,
        muted,
      })
    }

    if (items && items.batch.count > 0) {
      const batch = items.batch
      const moving =
        drag.ticks !== 0 ||
        drag.rows !== 0 ||
        drag.resizeStart !== 0 ||
        drag.resizeEnd !== 0
      // The same widening the grid does, so a clip dragged into view from
      // outside it gets its name too.
      const range = visibleRange(
        items,
        Math.min(
          ticks.start,
          ticks.start - drag.ticks - Math.max(0, drag.resizeEnd)
        ),
        Math.max(
          ticks.end,
          ticks.end - drag.ticks - Math.min(0, drag.resizeStart)
        )
      )
      for (let index = range.first; index < range.last; index++) {
        const id = batch.ids[index]
        const clip = clipById.get(id)
        if (!clip) continue
        const selected = batch.isSelected(index)
        let start = batch.start(index)
        let length = batch.length(index)
        let row = batch.row(index)
        const slip = this.source.slipDraft
        let offset = slip?.clip === id ? slip.offset : clip.offset
        if (selected && moving) {
          const resized = resizedSpan(
            start,
            length,
            drag.resizeStart,
            drag.resizeEnd,
            drag.minLength
          )
          // Moving the start edge leaves what is inside where it was. A
          // pattern loops, so its offset may go round; audio and curves
          // have nothing before their start.
          offset += resized.start - start
          if (clip.content.type !== "pattern") offset = Math.max(0, offset)
          start = resized.start + drag.ticks
          length = resized.length
          row += drag.rows
        }
        add(
          id,
          { start, length, offset },
          row,
          this.contentOf(clip),
          selected,
          this.source.isMuted(clip),
          false
        )
      }
    }

    for (const ghost of this.source.ghosts) {
      add(-1, ghost, ghost.row, ghost.content, false, ghost.muted, true)
    }
    return sprites.sort((a, b) => a.style - b.style)
  }

  private repaint(ctx: CanvasRenderingContext2D, frame: OverlayFrame): void {
    const { viewport, transform, theme } = frame
    if (theme !== this.theme) {
      this.styles.clear()
      this.theme = theme
      this.warn = themeColor("--wf-warn", "rgb(214,150,40)")
    }
    const dpr = viewport.dpr
    const lw = transform.lineWidth
    const width = transform.widthDev
    const height = transform.heightDev
    ctx.setTransform(1, 0, 0, 1, 0, 0)
    ctx.globalAlpha = 1
    ctx.clearRect(0, 0, width, height)

    this.paintHeldValues(ctx, frame)

    const sprites = this.collect(frame)
    const styleOf = (sprite: ClipSprite) => this.styles.get(sprite.style)
    const bandHeight = Math.round(BAND_HEIGHT * dpr)
    const hasBand = (sprite: ClipSprite) =>
      sprite.y1 - sprite.y0 >= MIN_HEIGHT_FOR_BAND * dpr &&
      sprite.x1 - sprite.x0 > 2 * lw

    // Bodies of clips that do not exist yet. Existing ones are in the grid.
    ctx.globalAlpha = 0.78
    for (const sprite of sprites) {
      if (!sprite.ghost) continue
      const style = styleOf(sprite)
      if (!style) continue
      const w = sprite.x1 - sprite.x0
      const h = sprite.y1 - sprite.y0
      ctx.fillStyle = style.border
      ctx.fillRect(sprite.x0, sprite.y0, w, h)
      if (w > 2 * lw && h > 2 * lw) {
        ctx.fillStyle = style.body
        ctx.fillRect(sprite.x0 + lw, sprite.y0 + lw, w - 2 * lw, h - 2 * lw)
      }
    }
    ctx.globalAlpha = 1

    let current = -1
    for (const sprite of sprites) {
      if (!hasBand(sprite)) continue
      if (sprite.style !== current) {
        current = sprite.style
        ctx.fillStyle = styleOf(sprite)?.band ?? ""
      }
      ctx.fillRect(
        sprite.x0 + lw,
        sprite.y0 + lw,
        sprite.x1 - sprite.x0 - 2 * lw,
        Math.min(bandHeight, sprite.y1 - sprite.y0 - 2 * lw)
      )
    }

    const ticks = visibleTicks(viewport)
    const pad = Math.round(2 * dpr)
    const maxNote = Math.round(MAX_NOTE_HEIGHT * dpr)
    let notesLeft = NOTE_BUDGET
    current = -1
    for (const sprite of sprites) {
      if (notesLeft <= 0) break
      const pattern = sprite.pattern
      if (!pattern) continue
      const top = sprite.y0 + lw + (hasBand(sprite) ? bandHeight : 0) + pad
      const area = sprite.y1 - lw - pad - top
      if (area < MIN_PREVIEW_HEIGHT * dpr) continue
      const preview = previewOf(pattern)
      // A pass needs room for its notes, or the preview is only noise.
      const room = Math.max(MIN_PASS_WIDTH, preview.count * NOTE_SPACING) * dpr
      if (preview.length * transform.scaleX < room) continue
      if (sprite.style !== current) {
        current = sprite.style
        ctx.fillStyle = styleOf(sprite)?.note ?? ""
      }
      const left = sprite.x0 + lw
      const right = sprite.x1 - lw
      notesLeft -= previewSpans(
        preview,
        sprite.span,
        ticks.start,
        ticks.end,
        (start, end, noteTop, noteHeight) => {
          const x0 = Math.max(left, deviceX(transform, start))
          let x1 = Math.min(right, deviceX(transform, end))
          // A hairline between neighbours keeps a row of steps countable.
          if (x1 - x0 > 3 * lw) x1 -= lw
          if (x1 <= x0) x1 = x0 + lw
          if (x1 > right) return
          const unit = noteHeight * area
          // Lines too thin for a gap between them fill their whole share.
          const h =
            unit < 3 * lw
              ? Math.max(lw, Math.floor(unit))
              : Math.min(maxNote, Math.floor(unit) - lw)
          const y = top + Math.round(noteTop * area + (unit - h) / 2)
          ctx.fillRect(x0, y, x1 - x0, h)
        }
      )
    }

    let dividersLeft = DIVIDER_BUDGET
    ctx.fillStyle = rgbaToCss(withAlpha(theme.foreground, 0.22))
    for (const sprite of sprites) {
      if (dividersLeft <= 0) break
      const pattern = sprite.pattern
      if (!pattern) continue
      const pass = patternTicks(pattern)
      if (pass * transform.scaleX < MIN_PASS_WIDTH * dpr) continue
      for (const tick of loopPoints(
        sprite.span,
        pass,
        ticks.start,
        ticks.end
      )) {
        const x = deviceX(transform, tick)
        if (x <= sprite.x0 + lw || x >= sprite.x1 - lw) continue
        ctx.fillRect(x, sprite.y0 + lw, lw, sprite.y1 - sprite.y0 - 2 * lw)
        dividersLeft--
      }
    }

    const columns = { columns: COLUMN_BUDGET }
    const marks = { marks: MARK_BUDGET }
    const missing = new Set<ClipSprite>()
    const inset = Math.round(CONTENT_PAD * dpr)
    for (const sprite of sprites) {
      const style = styleOf(sprite)
      if (!style) continue
      const area: Box = {
        left: sprite.x0 + lw,
        right: sprite.x1 - lw,
        top: sprite.y0 + lw + (hasBand(sprite) ? bandHeight : 0),
        bottom: sprite.y1 - lw,
      }
      const content = sprite.content
      if (content.type === "audio") {
        const peaks = this.source.peaksOf(content.sample)
        if (!peaks || peaks.status === "missing") {
          missing.add(sprite)
          paintMissing(ctx, frame, sprite, this.warn)
        }
        paintAudio(ctx, frame, sprite, style, {
          peaks,
          tempoBpm: this.source.tempoBpm,
          area,
          box: {
            left: sprite.x0 / dpr,
            right: sprite.x1 / dpr,
            top: sprite.y0 / dpr,
            bottom: sprite.y1 / dpr,
          },
          budget: columns,
        })
      } else if (content.type === "automation") {
        const draft = this.source.draft
        const points =
          draft && draft.automation === content.automation
            ? draft.points
            : this.source.lookups.automations.get(content.automation)?.points
        if (!points) continue
        paintAutomation(ctx, frame, sprite, style, {
          points,
          selected:
            this.source.pointSelection?.clip === sprite.id &&
            this.source.pointSelection.automation === content.automation
              ? this.source.pointSelection.indices
              : undefined,
          area: { ...area, top: area.top + inset, bottom: area.bottom - inset },
          range: this.source.viewOf(content.automation),
          labels: this.source.viewLabelsOf(content.automation),
          active: sprite.selected || sprite.id === this.source.hover,
          budget: marks,
        })
      }
    }

    this.setFont(ctx, dpr)
    ctx.textBaseline = "middle"
    const textPad = Math.round(TEXT_PAD * dpr)
    let named = 0
    for (const sprite of sprites) {
      const style = styleOf(sprite)
      const gone = missing.has(sprite)
      const name = !gone
        ? sprite.name
        : sprite.name === ""
          ? t("Missing sample")
          : message("Missing: {spriteName}", { spriteName: sprite.name })
      if (name === "" || !style) continue
      const h = sprite.y1 - sprite.y0
      if (h < MIN_LABEL_HEIGHT * dpr) continue
      if (sprite.x1 - sprite.x0 < MIN_LABEL_WIDTH * dpr) continue
      const band = hasBand(sprite)
      const linked =
        band &&
        sprite.content.type === "automation" &&
        this.source.shared.has(sprite.content.automation) &&
        sprite.x1 - sprite.x0 >= MIN_LINK_WIDTH * dpr
      // A clip that starts off-screen keeps its name at the left edge.
      const x = Math.max(sprite.x0, 0) + textPad
      // What sits at the right end of a title bar: the link glyph of a
      // shared curve, the gain handle of an audio clip.
      const handled =
        band &&
        !sprite.ghost &&
        sprite.content.type === "audio" &&
        sprite.x1 - sprite.x0 >= MIN_WIDTH_FOR_GAIN * dpr
      const glyphRoom = linked
        ? Math.round(14 * dpr)
        : handled
          ? Math.round((GAIN_HANDLE_WIDTH + GAIN_HANDLE_INSET) * dpr)
          : 0
      const middle = band ? sprite.y0 + lw + bandHeight / 2 : sprite.y0 + h / 2
      if (linked) {
        paintLinkGlyph(
          ctx,
          Math.min(sprite.x1, width) - Math.round(9 * dpr),
          Math.round(middle),
          dpr,
          style.bandInk
        )
      }
      const text = this.fit(ctx, name, sprite.x1 - textPad - x - glyphRoom, dpr)
      if (text === "") continue
      // A missing sample is said in the warning color where there is no
      // title bar to carry it.
      ctx.fillStyle = band ? style.bandInk : gone ? this.warn : style.bodyInk
      ctx.fillText(text, x, Math.round(middle) + 0.5 * dpr)
      named++
    }

    const { marked } = this.source
    if (marked.size > 0) {
      ctx.fillStyle = rgbaToCss(withAlpha(theme.background, 0.66))
      for (const sprite of sprites) {
        if (!marked.has(sprite.id)) continue
        ctx.fillRect(
          sprite.x0,
          sprite.y0,
          sprite.x1 - sprite.x0,
          sprite.y1 - sprite.y0
        )
      }
    }

    if (this.source.cloneHint) this.paintOrigins(ctx, frame)
    this.paintSongEnd(ctx, frame)
    this.paintDropPreview(ctx, frame)
    this.paintBadge(ctx, frame)

    this.stats.clips = named
    this.stats.notes = NOTE_BUDGET - notesLeft
    this.stats.columns = COLUMN_BUDGET - columns.columns
  }

  /** The dashed lines that show a target staying where a clip left it. */
  private paintHeldValues(
    ctx: CanvasRenderingContext2D,
    frame: OverlayFrame
  ): void {
    const { holds } = this.source
    if (holds.length === 0) return
    const automations = this.source.lookups.automations
    paintHolds(
      ctx,
      frame,
      holds,
      (track) => this.source.trackRow(track),
      (hold) => automations.get(hold.automation)?.color ?? ORPHAN_COLOR,
      (hold) => this.source.viewOf(hold.automation)
    )
  }

  /**
   * The selected clips where they are stored, as faint boxes under a drag.
   * With Shift held they will stay there, and this is what says so.
   */
  private paintOrigins(
    ctx: CanvasRenderingContext2D,
    frame: OverlayFrame
  ): void {
    const { viewport, transform, theme } = frame
    const items = this.source.items
    if (!items || items.batch.selectedCount === 0) return
    const lw = transform.lineWidth
    const ticks = visibleTicks(viewport)
    const range = visibleRange(items, ticks.start, ticks.end)
    const wash = rgbaToCss(withAlpha(theme.foreground, 0.14))
    const outline = rgbaToCss(withAlpha(theme.foreground, 0.55))
    for (let index = range.first; index < range.last; index++) {
      if (!items.batch.isSelected(index)) continue
      const x0 = deviceX(transform, items.batch.start(index))
      const x1 = Math.max(x0 + lw, deviceX(transform, items.batch.end(index)))
      const y0 =
        playlistDeviceY(viewport, transform, items.batch.row(index)) + lw
      const y1 = playlistDeviceY(
        viewport,
        transform,
        items.batch.row(index) + 1
      )
      ctx.fillStyle = wash
      ctx.fillRect(x0, y0, x1 - x0, y1 - y0)
      ctx.fillStyle = outline
      ctx.fillRect(x0, y0, x1 - x0, lw)
      ctx.fillRect(x0, y1 - lw, x1 - x0, lw)
      ctx.fillRect(x0, y0, lw, y1 - y0)
      ctx.fillRect(x1 - lw, y0, lw, y1 - y0)
    }
  }

  /** A dashed line where the song ends. */
  private paintSongEnd(
    ctx: CanvasRenderingContext2D,
    frame: OverlayFrame
  ): void {
    const { viewport, transform, theme } = frame
    const end = this.source.songEnd
    if (end <= 0) return
    const x = deviceX(transform, end)
    if (x < 0 || x >= transform.widthDev) return
    const dash = Math.round(4 * viewport.dpr)
    ctx.fillStyle = rgbaToCss(withAlpha(theme.mutedForeground, 0.6))
    for (let y = 0; y < transform.heightDev; y += dash * 2) {
      ctx.fillRect(x, y, transform.lineWidth, dash)
    }
  }

  /** Where a file dragged over the timeline would land, with its name. */
  private paintDropPreview(
    ctx: CanvasRenderingContext2D,
    frame: OverlayFrame
  ): void {
    const preview = this.source.dropPreview
    if (!preview) return
    const { viewport, transform, theme } = frame
    const dpr = viewport.dpr
    const lw = transform.lineWidth
    const x0 = deviceX(transform, preview.start)
    const x1 = Math.max(
      x0 + Math.round(24 * dpr),
      deviceX(transform, preview.start + preview.length)
    )
    const y0 = playlistDeviceY(viewport, transform, preview.row) + lw
    const y1 = playlistDeviceY(viewport, transform, preview.row + 1)
    ctx.fillStyle = rgbaToCss(withAlpha(theme.playhead, 0.2))
    ctx.fillRect(x0, y0, x1 - x0, y1 - y0)
    ctx.fillStyle = rgbaToCss(theme.playhead)
    const edge = Math.max(lw, Math.round(1.5 * dpr))
    ctx.fillRect(x0, y0, x1 - x0, edge)
    ctx.fillRect(x0, y1 - edge, x1 - x0, edge)
    ctx.fillRect(x0, y0, edge, y1 - y0)
    ctx.fillRect(x1 - edge, y0, edge, y1 - y0)
    if (y1 - y0 < MIN_LABEL_HEIGHT * dpr) return
    this.setFont(ctx, dpr)
    ctx.textBaseline = "middle"
    const pad = Math.round(TEXT_PAD * dpr)
    const text = this.fit(ctx, preview.name, x1 - x0 - 2 * pad, dpr)
    ctx.fillStyle = rgbaToCss(theme.foreground)
    ctx.fillText(text, x0 + pad + edge, Math.round((y0 + y1) / 2))
  }

  /** The value of what is being dragged, in a small box by the pointer. */
  private paintBadge(ctx: CanvasRenderingContext2D, frame: OverlayFrame): void {
    const badge = this.source.badge
    if (!badge) return
    const { viewport, transform, theme } = frame
    const dpr = viewport.dpr
    this.setFont(ctx, dpr)
    ctx.textBaseline = "middle"
    const padX = Math.round(6 * dpr)
    const boxHeight = Math.round(18 * dpr)
    const boxWidth = Math.ceil(ctx.measureText(badge.text).width) + 2 * padX
    // Up and to the right of the pointer, and kept inside the grid.
    const x = Math.round(badge.x * dpr)
    const y = Math.round(badge.y * dpr)
    const left = Math.min(
      Math.max(0, x + Math.round(12 * dpr)),
      Math.max(0, transform.widthDev - boxWidth)
    )
    const above = y - boxHeight - Math.round(10 * dpr)
    const top = above < 0 ? y + Math.round(14 * dpr) : above
    ctx.fillStyle = rgbaToCss(theme.foreground)
    ctx.fillRect(left, top, boxWidth, boxHeight)
    ctx.fillStyle = rgbaToCss(theme.background)
    ctx.fillText(badge.text, left + padX, top + boxHeight / 2 + 0.5 * dpr)
  }

  private setFont(ctx: CanvasRenderingContext2D, dpr: number): void {
    const font = `600 ${Math.round(FONT_SIZE * dpr * 10) / 10}px "Inter Variable", system-ui, sans-serif`
    if (font !== this.font) {
      this.font = font
      this.widths.clear()
      this.fits.clear()
    }
    ctx.font = font
  }

  /** `text`, cut short with an ellipsis when it is wider than `room`. */
  private fit(
    ctx: CanvasRenderingContext2D,
    text: string,
    room: number,
    dpr: number
  ): string {
    if (room <= 0) return ""
    let full = this.widths.get(text)
    if (full === undefined) {
      full = ctx.measureText(text).width
      this.widths.set(text, full)
    }
    if (full <= room) return text
    // Measuring is slow, so widths are rounded down to a few pixels and
    // each rounded width is worked out once per name.
    const bucket = Math.floor(room / (FIT_BUCKET * dpr))
    const key = `${bucket}\n${text}`
    const cached = this.fits.get(key)
    if (cached !== undefined) return cached
    const limit = bucket * FIT_BUCKET * dpr
    let low = 0
    let high = text.length - 1
    while (low < high) {
      const middle = Math.ceil((low + high) / 2)
      const candidate = `${text.slice(0, middle).trimEnd()}…`
      if (ctx.measureText(candidate).width <= limit) low = middle
      else high = middle - 1
    }
    // One or two letters and an ellipsis say nothing.
    const fitted = low >= 3 ? `${text.slice(0, low).trimEnd()}…` : ""
    if (this.fits.size > 4000) this.fits.clear()
    this.fits.set(key, fitted)
    return fitted
  }
}
