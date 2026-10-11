import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent,
} from "react"

import {
  ContextActions,
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import {
  plainRows,
  type TimeGridView,
  type TimeGridViewOptions,
} from "@/lib/canvas"
import { TimeGridCanvas } from "@/lib/canvas/react"
import { activeSampleDrag, hasSampleDrag, readSampleDrag } from "@/lib/dnd"
import { activeSelectionDrag, hasSelectionDrag, readSelectionDrag, transferSelection } from "@/features/audio-editor/transfer"
import { wholeClipTicks } from "./audio/geometry"
import { MODIFIER_HINTS } from "@/lib/edit-modifiers"
import { errorMessage } from "@/lib/ipc"
import { useHint } from "@/lib/store/hint"
import { useProjectStore } from "@/lib/store/project"
import { usePlayhead } from "@/lib/store/realtime"
import { usePattern, useSelectedPatternId } from "@/lib/store/selectors"
import { useTransportStore } from "@/lib/store/transport"

import { setActiveSession } from "./active"
import { fileClipTicks } from "./audio/drop"
import { addAudioFile } from "./audio/ops"
import { pointMenu, selectedClipLfoItems } from "./automation/menu"
import { selectedViewRangeItems } from "./automation/view-menu"
import type { InnerHit } from "./inner"
import type { Tool } from "./intents"
import { MIN_ROWS } from "./layout"
import { initialView, PLAYLIST_LIMITS, type GridMetrics } from "./metrics"
import { attachPointer } from "./pointer"
import { viewportShowing } from "./reveal"
import { playlist, project, useClipCount } from "./selectors"
import { PlaylistSession } from "./session"
import { gridSpecFor } from "./snap"
import { usePlaylistStore } from "./store"
import { TrackGridSurface } from "./track-grid-surface"
import { documentRowFor, trackRowsNow } from "./track-rows"

const CLIP_MENU: ContextItem[] = [
  "playlist.editPattern",
  "playlist.reverseClips",
  "playlist.compAudio",
  "playlist.compTakeGroup",
  "playlist.groupAudioTakes",
  contextSeparator,
  "playlist.cut",
  "playlist.copy",
  "playlist.selectMatchingClips",
  "playlist.duplicate",
  "playlist.makeUnique",
  "playlist.muteClips",
  "playlist.bounceSelectedClips",
  "playlist.consolidateSelectedTracks",
  contextSeparator,
  "playlist.clipInspector",
  // How much of its range an automation clip shows, when one is selected.
  { dynamic: selectedViewRangeItems },
  // The LFO writer, for one selected automation clip.
  { dynamic: selectedClipLfoItems },
  contextSeparator,
  "playlist.deleteClips",
]

const EMPTY_MENU: ContextItem[] = [
  "playlist.paste",
  "playlist.selectAll",
  contextSeparator,
  "playlist.zoomToFit",
  "playlist.tallTracks",
  "playlist.addTrack",
]

/**
 * The status bar's line for a tool: what a click does, what a drag does and
 * the modifier keys, and no more. The line has to fit beside the engine and
 * the file's state, so every word in it has to be worth its place.
 */
export function hintFor(tool: Tool, pattern: string): string {
  const { copy, add, free } = MODIFIER_HINTS
  const edit = `Drag clips to move, edges to resize. ${copy}, ${add}, ${free}`
  switch (tool) {
    case "draw":
      return `Click to place ${pattern}. ${edit}. Right-click deletes`
    case "paint":
      return `Drag to paint ${pattern}. ${edit}. Right-click deletes`
    case "select":
      return `Drag a box to select. ${edit}. Right-click for a menu`
    case "erase":
      return "Click or drag across clips to delete them"
    case "mute":
      return "Click or drag across clips to mute or unmute. Right-click deletes"
    case "slip":
      return `Drag content inside a fixed clip. ${free}. Right-click deletes`
    case "playback":
      return `Click or drag to seek the transport. ${free}. Escape ends scrubbing`
    case "slice":
      return `Drag a vertical cut, release to split clips. ${free}. Escape cancels`
    default: {
      const _exhaustive: never = tool
      return _exhaustive
    }
  }
}

/** What the status bar says about the part of a clip under the pointer. */
export const INNER_HINTS: Record<InnerHit["kind"], string> = {
  fade: "Drag to set the fade's length. Alt: no snap",
  gain: "Drag up or down for the clip's gain. Shift: fine",
  point:
    "Drag to move the point. Shift: one axis, Alt: no snap. Double-click holds, right-click deletes",
  bend: "Drag up or down to bend this stretch of the curve",
  curve:
    "Click to add a point and drag it. Alt: no snap. The title bar moves the clip",
}

/** How far from the left edge the playhead lands when the view follows it. */
const FOLLOW_LEAD = 0.1

/** What the grid's right-click menu offers for where it was opened. */
function gridMenu(): ContextItem[] {
  const { menuPoint, menuOnClips } = usePlaylistStore.getState()
  if (menuPoint) {
    const items = pointMenu(menuPoint)
    if (items.length > 0) return items
  }
  return menuOnClips ? CLIP_MENU : EMPTY_MENU
}

/**
 * The clip grid: the canvas, the session that keeps it current and turns
 * the pointer into edits, the playhead, the right-click menu of the Select
 * tool, and the place sounds dragged from the browser are dropped on.
 */
export function PlaylistGrid({ metrics }: { metrics: GridMetrics }) {
  const [failure, setFailure] = useState<string | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const viewRef = useRef<TimeGridView | null>(null)
  const sessionRef = useRef<PlaylistSession | null>(null)
  const cleanupRef = useRef<(() => void) | null>(null)
  const tool = usePlaylistStore((state) => state.tool)
  const brush = usePlaylistStore((state) => state.brush)
  const clipCount = useClipCount()
  const pattern = usePattern(useSelectedPatternId())
  const brushName = useProjectStore((state) =>
    brush.type === "audio"
      ? state.project.samples.find((item) => item.id === brush.sample)?.name
      : brush.type === "automation"
        ? state.project.automations.find((item) => item.id === brush.automation)
            ?.name
        : undefined
  )
  const focusRequested = usePlaylistStore((state) => state.focusRequested)
  // Also on mount: the request may be older than the panel.
  useEffect(() => {
    if (!focusRequested) return
    containerRef.current?.focus({ preventScroll: true })
    usePlaylistStore.getState().focusGiven()
  }, [focusRequested])
  const [inner, setInner] = useState<InnerHit["kind"] | null>(null)
  const hint = useHint(
    inner
      ? INNER_HINTS[inner]
      : hintFor(tool, brushName ?? pattern?.name ?? "a pattern")
  )

  // The view reads its options once. The session sets everything that can
  // change afterwards, so these are only the values to start with.
  const options = useMemo<TimeGridViewOptions>(
    () => ({
      renderer: "auto",
      limits: PLAYLIST_LIMITS,
      rows: plainRows(MIN_ROWS),
      timeGrid: gridSpecFor(usePlaylistStore.getState().snap, {
        numerator: 4,
        denominator: 4,
      }),
      initial: initialView(),
    }),
    []
  )

  /** Scrolls to the clip another panel asked to be shown. */
  const showRevealed = useCallback(() => {
    const id = usePlaylistStore.getState().reveal
    if (id === null) return
    const { clips } = playlist()
    const clip = clips.find((item) => item.id === id)
    if (clip) {
      const row = trackRowsNow().trackRows.get(clip.track)
      if (row === undefined) {
        usePlaylistStore.getState().setReveal(null)
        return
      }
      // A curve needs a tall row to be drawn in.
      if (clip.content.type === "automation" && !metrics.tall) {
        metrics.toggleTall()
      }
      metrics.setViewport(viewportShowing(metrics.viewport, clip, row))
    }
    usePlaylistStore.getState().setReveal(null)
  }, [metrics])

  const onReady = useCallback(
    (view: TimeGridView) => {
      const surface = new TrackGridSurface(view)
      const detach = metrics.attach(surface)
      const session = new PlaylistSession(surface, metrics)
      session.onCursor = (cursor) => {
        view.element.style.cursor = cursor
      }
      session.onInner = setInner
      const stops = [
        attachPointer(view.element, session, metrics, {
          localPoint: (event) => view.localPoint(event),
          focus: () => containerRef.current?.focus({ preventScroll: true }),
        }),
        setActiveSession(session),
        usePlaylistStore.subscribe((state, previous) => {
          if (state.reveal !== null && state.reveal !== previous.reveal) {
            showRevealed()
          }
        }),
        () => session.destroy(),
        detach,
        () => surface.destroy(),
      ]
      viewRef.current = view
      sessionRef.current = session
      // Says which renderer drew the grid, for bug reports and measurements.
      containerRef.current?.setAttribute(
        "data-renderer",
        view.renderer.info.kind
      )
      cleanupRef.current = () => {
        for (const stop of stops) stop()
        viewRef.current = null
        sessionRef.current = null
      }
      showRevealed()
    },
    [metrics, showRevealed]
  )

  const onError = useCallback((error: unknown) => {
    setFailure(errorMessage(error))
  }, [])

  useEffect(
    () => () => {
      cleanupRef.current?.()
      cleanupRef.current = null
    },
    []
  )

  usePlayhead((tick, playing) => {
    const view = viewRef.current
    const transport = useTransportStore.getState()
    const ui = usePlaylistStore.getState()
    if (transport.mode !== "song") {
      // The playhead is inside the pattern now, not on the playlist.
      view?.setPlayhead(null)
      return
    }
    view?.setPlayhead(tick)
    // While stopped the playhead rests where the song will start from.
    if (!playing) ui.setCursorTick(tick)
    if (!playing || !ui.follow) return
    const viewport = metrics.viewport
    const visible = viewport.width / viewport.pxPerTick
    if (tick < viewport.scrollTick || tick > viewport.scrollTick + visible) {
      metrics.setViewport({
        ...viewport,
        scrollTick: tick - visible * FOLLOW_LEAD,
      })
    }
  })

  /** Shows where a dragged sound would land, and says a drop is welcome. */
  function previewDrop(event: DragEvent<HTMLDivElement>) {
    const view = viewRef.current
    const session = sessionRef.current
    // Anything that is not a sound is left to the browser, which shows it
    // cannot be dropped here.
    if (!view || !session || (!hasSampleDrag(event) && !hasSelectionDrag(event))) return null
    event.preventDefault()
    event.dataTransfer.dropEffect = "copy"
    const sample = activeSampleDrag()
    const at = { ...view.localPoint(event), alt: event.altKey }
    const tempo = project().settings.tempoBpm
    const selection=activeSelectionDrag()
    if (hasSelectionDrag(event)) return session.previewDrop(at,"Editor selection",selection ? wholeClipTicks(selection.durationSecs,tempo) : null)
    const length = () => (sample ? fileClipTicks(sample.path, tempo) : null)
    if (sample) {
      fileClipTicks(sample.path, tempo, () => {
        // The preview grows to the file's real length once it is read.
        if (session.dropPreview) session.previewDrop(at, sample.name, length())
      })
    }
    return session.previewDrop(at, sample?.name ?? "Sound", length())
  }

  function onDrop(event: DragEvent<HTMLDivElement>) {
    const place = previewDrop(event)
    sessionRef.current?.previewDrop(null)
    const selection=readSelectionDrag(event)
    if (place && selection) {
      const row=documentRowFor(place.row)
      if (row===undefined) return
      void transferSelection(selection,{type:"playlist",track:playlist().tracks[row]?.id ?? null,start:place.start})
      return
    }
    const sample = readSampleDrag(event)
    if (!place || !sample) return
    const row = documentRowFor(place.row)
    if (row === undefined) return
    containerRef.current?.focus({ preventScroll: true })
    void addAudioFile(sample.path, { ...place, row }, sample.browser)
  }

  return (
    <ContextActions items={gridMenu}>
      <div
        ref={containerRef}
        tabIndex={0}
        role="application"
        aria-label="Song timeline"
        aria-roledescription="clip grid"
        data-slot="playlist-grid"
        data-tool={tool}
        className="focus-frame relative min-h-0 min-w-0 touch-none overflow-hidden"
        onDragOver={previewDrop}
        onDragLeave={() => sessionRef.current?.previewDrop(null)}
        onDrop={onDrop}
        {...hint}
      >
        <TimeGridCanvas
          className="absolute inset-0"
          options={options}
          onReady={onReady}
          onError={onError}
        />
        {failure !== null && (
          <p
            role="alert"
            className="absolute inset-x-0 top-1/3 mx-auto max-w-sm text-center text-muted-foreground"
          >
            The timeline could not be drawn. {failure}
          </p>
        )}
        {clipCount === 0 && failure === null && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center p-6">
            <div className="max-w-xs rounded-lg border bg-popover/85 px-4 py-3 text-center shadow-xs">
              <p className="font-medium text-foreground">The song is empty</p>
              <p className="mt-1 text-muted-foreground">
                Pick a pattern on the left and click in the timeline to place
                it, or drag a sound in from the browser. Every row can hold
                clips; tracks are added as you go.
              </p>
            </div>
          </div>
        )}
      </div>
    </ContextActions>
  )
}
