import { logicalDelta } from "@/lib/ui-scale"
import { Add01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import {
  memo,
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react"

import type { PlaylistTrack, TrackGroup } from "@/bindings"
import { ToggleLed } from "@/components/audio"
import { ActionButton } from "@/components/action-button"
import {
  ContextActions,
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import { TRACK_COLORS } from "@/features/mixer/colors"
import { useHint } from "@/lib/store/hint"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { colorToCss } from "@/lib/units"
import { cn } from "@/lib/utils"

import { trackDropIndex } from "./edit"
import { linkMenu, selectLinkedSource, trackLinkLabel } from "./linked-tracks"
import { DRAG_THRESHOLD_PX, HEADER_WIDTH, MIN_ROW_HEIGHT } from "./layout"
import { useViewportValue, wheelInput, type GridMetrics } from "./metrics"
import {
  moveTrack,
  setTrackColor,
  setTrackName,
  toggleTrackMute,
  toggleTrackSolo,
} from "./ops"
import { playlistYToRow, rowGeometry } from "./row-geometry"
import { playlist } from "./selectors"
import {
  beginTrackResize,
  endTrackResize,
  syncTrackHeights,
  usePlaylistStore,
} from "./store"
import { toggleGroupMute, toggleGroupSolo } from "./track-group-ops"
import { nextTrackHeightPreset } from "./track-height-preset-step"
import { nextTrackHeight, TRACK_HEIGHT_PRESETS } from "./track-height-presets"
import { nextTrackHeightScale } from "./track-height-scale"
import { trackRowsNow, useTrackRows } from "./track-rows"
import { message, translatedText as t, dynamicTitle, plainNumber } from "@/lib/i18n"

/** A header's menu acts on the target track; a press on the header sets it. */
const TRACK_MENU: ContextItem[] = [
  "playlist.renameTrack",
  "playlist.selectTrackClips",
  "playlist.consolidateTrack",
  "playlist.muteTrack",
  contextSeparator,
  "playlist.moveTrackUp",
  "playlist.moveTrackDown",
  contextSeparator,
  "playlist.insertTrack",
  "playlist.addTrack",
  "playlist.addInstrumentTrack",
  "playlist.addAudioTrack",
  contextSeparator,
  "playlist.deleteTrack",
]

const SPARE_MENU: ContextItem[] = [
  "playlist.addTrack",
  "playlist.addInstrumentTrack",
  "playlist.addAudioTrack",
]

function NameField({
  track,
  onDone,
}: {
  track: PlaylistTrack
  onDone(): void
}) {
  const [text, setText] = useState(track.name)
  const cancelled = useRef(false)
  return (
    <input
      autoFocus
      aria-label={message("Name of {trackName}", { trackName: track.name })}
      value={text}
      maxLength={64}
      onChange={(event) => setText(event.target.value)}
      onFocus={(event) => event.target.select()}
      onBlur={() => {
        if (!cancelled.current) void setTrackName(track.id, text)
        onDone()
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") event.currentTarget.blur()
        if (event.key === "Escape") {
          cancelled.current = true
          event.currentTarget.blur()
        }
      }}
      className="h-5 min-w-0 flex-1 rounded-sm border border-ring bg-background px-1 text-xs outline-none"
    />
  )
}

type TrackHeaderProps = {
  track: PlaylistTrack
  row: number
  index: number
  depth: number
  top: number
  height: number
  target: boolean
  /** The header is being dragged to another place among the tracks. */
  lifted: boolean
  onGrab(event: ReactPointerEvent<HTMLElement>, row: number): void
}

const TrackHeader = memo(function TrackHeader({
  track,
  row,
  index,
  depth,
  top,
  height,
  target,
  lifted,
  onGrab,
}: TrackHeaderProps) {
  const [renaming, setRenaming] = useState(false)
  const resize = useRef<{
    pointer: number
    y: number
    height: number
    generation: number
  } | null>(null)
  const cancelResize = useCallback(() => {
    if (!resize.current) return
    resize.current = null
    endTrackResize(track.id)
    const saved = playlist().tracks.find((item) => item.id === track.id)
    usePlaylistStore.getState().setTrackHeight(track.id, saved?.height ?? 0)
  }, [track.id])
  useEffect(() => cancelResize, [cancelResize])
  const resizeHint = useHint(message("Drag to resize {trackName}", { trackName: track.name }))
  const linkLabel = useProjectStore(({ project }) =>
    trackLinkLabel(project, track.id)
  )
  const setTargetTrack = usePlaylistStore((state) => state.setTargetTrack)
  const hint = useHint(
    message(
      track.muted && track.solo
        ? "{trackName}, muted, solo. Drag up or down to reorder, double-click the name to rename, right-click for more"
        : track.muted
          ? "{trackName}, muted. Drag up or down to reorder, double-click the name to rename, right-click for more"
          : track.solo
            ? "{trackName}, solo. Drag up or down to reorder, double-click the name to rename, right-click for more"
            : "{trackName}. Drag up or down to reorder, double-click the name to rename, right-click for more",
      { trackName: track.name }
    )
  )
  const lampHint = useHint(
    message(
      track.muted
        ? "{trackName} is muted. Click to unmute every clip on it"
        : "{trackName} is on. Click to mute every clip on it",
      { trackName: track.name }
    )
  )
  const soloHint = useHint(
    message(
      track.solo
        ? "{trackName} is solo. Click to unsolo this track. Mute still wins"
        : "{trackName} is not solo. Click to solo this track. Mute still wins",
      { trackName: track.name }
    )
  )
  const compact = height < 22

  return (
    <ContextActions
      items={() => [
        ...TRACK_MENU,
        contextSeparator,
        linkMenu(track.id),
        {
          submenu: t("Color"),
          items: [
            {
              ...dynamicTitle("No color", {}),
              checked: !track.color,
              run: () => setTrackColor(track.id, 0),
            },
            ...TRACK_COLORS.map(({ color, name }) => ({
              title: name,
              checked: track.color === color,
              run: () => setTrackColor(track.id, color),
            })),
          ],
        },
        {
          submenu: t("Track height"),
          items: [
            ...TRACK_HEIGHT_PRESETS.map(({ label, height: preset }) => {
              const height = nextTrackHeight(track.height ?? 0, preset)
              return {
                title: label,
                checked: height === null,
                disabled: height === null,
                run: () => {
                  const height = nextTrackHeight(track.height ?? 0, preset)
                  if (height !== null) {
                    return dispatch({
                      type: "updatePlaylistTrack",
                      id: track.id,
                      patch: { height },
                    })
                  }
                },
              }
            }),
            {
              ...dynamicTitle("Previous preset", {}),
              disabled:
                nextTrackHeightPreset(track.height ?? 0, "previous") === null,
              run: () => {
                const saved = useProjectStore
                  .getState()
                  .project.playlist.tracks.find((item) => item.id === track.id)
                const next = nextTrackHeightPreset(saved?.height ?? 0, "previous")
                if (next === null) return
                return dispatch({
                  type: "updatePlaylistTrack",
                  id: track.id,
                  patch: { height: next },
                })
              },
            },
            {
              ...dynamicTitle("Next preset", {}),
              disabled:
                nextTrackHeightPreset(track.height ?? 0, "next") === null,
              run: () => {
                const saved = useProjectStore
                  .getState()
                  .project.playlist.tracks.find((item) => item.id === track.id)
                const next = nextTrackHeightPreset(saved?.height ?? 0, "next")
                if (next === null) return
                return dispatch({
                  type: "updatePlaylistTrack",
                  id: track.id,
                  patch: { height: next },
                })
              },
            },
            {
              ...dynamicTitle("Half", {}),
              disabled: nextTrackHeightScale(track.height ?? 0, "half") === null,
              run: () => {
                const saved = useProjectStore
                  .getState()
                  .project.playlist.tracks.find((item) => item.id === track.id)
                const next = nextTrackHeightScale(saved?.height ?? 0, "half")
                if (next !== null) {
                  return dispatch({
                    type: "updatePlaylistTrack",
                    id: track.id,
                    patch: { height: next },
                  })
                }
              },
            },
            {
              ...dynamicTitle("Double", {}),
              disabled: nextTrackHeightScale(track.height ?? 0, "double") === null,
              run: () => {
                const saved = useProjectStore
                  .getState()
                  .project.playlist.tracks.find((item) => item.id === track.id)
                const next = nextTrackHeightScale(saved?.height ?? 0, "double")
                if (next !== null) {
                  return dispatch({
                    type: "updatePlaylistTrack",
                    id: track.id,
                    patch: { height: next },
                  })
                }
              },
            },
          ],
        },
      ]}
    >
      <div
        role="group"
        aria-label={track.name}
        data-track={track.id}
        data-solo={track.solo ? "" : undefined}
        data-target={target ? "" : undefined}
        data-lifted={lifted ? "" : undefined}
        onPointerDown={(event) => {
          setTargetTrack(track.id)
          // An instrument or audio track brings its channel and mixer
          // track into view with it, as a conventional track does.
          selectLinkedSource(track.id)
          if (!renaming) onGrab(event, row)
        }}
        style={{
          top,
          height,
          paddingLeft: depth > 0 ? 4 + depth * 12 : undefined,
          backgroundColor: track.color
            ? `${colorToCss(track.color)}2e`
            : undefined,
        }}
        className={cn(
          "absolute inset-x-0 flex items-center gap-1.5 border-t border-(--wf-grid-line) pr-1.5 pl-1 data-lifted:opacity-45",
          target ? "bg-accent/70" : "hover:bg-accent/30"
        )}
        {...hint}
      >
        <span
          aria-hidden
          className="w-5 shrink-0 text-right font-readout text-[0.625rem] text-muted-foreground"
        >
          {index + 1}
        </span>
        <ToggleLed
          variant="dot"
          size={compact ? "sm" : "md"}
          pressed={!track.muted}
          color="var(--wf-meter-low)"
          aria-label={message("{trackName} on", { trackName: track.name })}
          onPressedChange={() => void toggleTrackMute(track.id)}
          {...lampHint}
        />
        <ToggleLed
          size={compact ? "sm" : "md"}
          pressed={track.solo ?? false}
          color="var(--wf-solo, var(--wf-meter-low))"
          aria-label={message("Solo {trackName}", { trackName: track.name })}
          onPressedChange={() => void toggleTrackSolo(track.id)}
          {...soloHint}
        >
          S
        </ToggleLed>
        {renaming ? (
          <NameField track={track} onDone={() => setRenaming(false)} />
        ) : (
          <span
            onDoubleClick={() => setRenaming(true)}
            className={cn(
              "min-w-0 flex-1 truncate",
              track.muted && "text-muted-foreground"
            )}
          >
            {track.name}
            {track.solo && (
              <span className="text-muted-foreground">{" "}{t("· Solo")}</span>
            )}
            {linkLabel !== null && (
              <span title={linkLabel} className="text-muted-foreground">
                {" · "}
                {linkLabel}
              </span>
            )}
          </span>
        )}
        <div
          aria-label={message("Resize {trackName}", { trackName: track.name })}
          data-slot="track-resize-handle"
          className="absolute inset-x-0 bottom-0 z-10 h-1 cursor-row-resize touch-none"
          onPointerDown={(event) => {
            event.stopPropagation()
            if (event.button !== 0) return
            event.preventDefault()
            resize.current = {
              pointer: event.pointerId,
              y: event.clientY,
              height,
              generation: getProjectGeneration(),
            }
            beginTrackResize(track.id)
            event.currentTarget.setPointerCapture(event.pointerId)
          }}
          onPointerMove={(event) => {
            const held = resize.current
            if (!held || held.pointer !== event.pointerId) return
            if (held.generation !== getProjectGeneration()) {
              cancelResize()
              return
            }
            usePlaylistStore
              .getState()
              .setTrackHeight(
                track.id,
                Math.max(
                  MIN_ROW_HEIGHT,
                  held.height + logicalDelta(event.clientY - held.y)
                )
              )
          }}
          onPointerUp={(event) => {
            const held = resize.current
            if (!held || held.pointer !== event.pointerId) return
            if (held.generation !== getProjectGeneration()) {
              cancelResize()
              return
            }
            resize.current = null
            endTrackResize(track.id)
            event.currentTarget.releasePointerCapture(event.pointerId)
            const saved = playlist().tracks.find((item) => item.id === track.id)
            const height =
              usePlaylistStore.getState().trackHeights.get(track.id) ?? 0
            if (saved && height !== (saved.height ?? 0)) {
              void dispatch({
                type: "updatePlaylistTrack",
                id: track.id,
                patch: { height },
              })
            }
          }}
          onPointerCancel={(event) => {
            if (resize.current?.pointer === event.pointerId) cancelResize()
          }}
          onLostPointerCapture={cancelResize}
          {...resizeHint}
        />
      </div>
    </ContextActions>
  )
})

const GroupHeader = memo(function GroupHeader({
  group,
  members,
  depth,
  top,
  height,
}: {
  group: TrackGroup
  members: readonly PlaylistTrack[]
  depth: number
  top: number
  height: number
}) {
  const collapsed = usePlaylistStore((state) =>
    state.collapsedGroups.has(group.id)
  )
  const toggleCollapse = usePlaylistStore((state) => state.toggleGroupCollapse)
  const on = members.some((track) => !track.muted)
  const solo = members.length > 0 && members.every((track) => track.solo)
  return (
    <div
      role="group"
      aria-label={group.name}
      data-track-group={group.id}
      style={{ top, height, paddingLeft: 4 + depth * 12 }}
      className="absolute inset-x-0 flex items-center gap-1.5 border-t border-(--wf-grid-line) bg-accent/40 pr-1.5 font-medium"
    >
      <button
        type="button"
        aria-label={message(collapsed ? "Expand {groupName}" : "Collapse {groupName}", { groupName: group.name })}
        aria-expanded={!collapsed}
        onClick={() => toggleCollapse(group.id)}
        className="w-5 shrink-0 rounded-sm text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring"
      >
        <span aria-hidden>{collapsed ? "▸" : "▾"}</span>
      </button>
      <ToggleLed
        variant="dot"
        size={height < 22 ? "sm" : "md"}
        pressed={on}
        disabled={members.length === 0}
        color="var(--wf-meter-low)"
        aria-label={message(on ? "Mute {groupName} tracks" : "Unmute {groupName} tracks", { groupName: group.name })}
        onPressedChange={() => void toggleGroupMute(group.id)}
      />
      <ToggleLed
        size={height < 22 ? "sm" : "md"}
        pressed={solo}
        disabled={members.length === 0}
        color="var(--wf-solo, var(--wf-meter-low))"
        aria-label={message("Solo {groupName}", { groupName: group.name })}
        onPressedChange={() => void toggleGroupSolo(group.id)}
      >
        S
      </ToggleLed>
      <span className="min-w-0 flex-1 truncate">{group.name}</span>
    </div>
  )
})

/** A header being dragged: the row it came from and the gap it is over. */
type Reorder = { row: number; gap: number }
type Grab = { row: number; y: number; pointer: number }

/** Translate a visible header gap back to the document's track order. */
function trackDrop(row: number, gap: number) {
  const { rows } = trackRowsNow()
  const source = rows[row]
  if (source?.kind !== "track") return null
  const next = rows.slice(gap).find((entry) => entry.kind === "track")
  const before = rows.slice(0, gap).findLast((entry) => entry.kind === "track")
  const documentGap =
    next?.kind === "track"
      ? next.index
      : before?.kind === "track"
        ? before.index + 1
        : playlist().tracks.length
  const index = trackDropIndex(
    source.index,
    documentGap,
    playlist().tracks.length
  )
  return index === null ? null : { track: source.track.id, index }
}

/** A row with no track yet. Placing a clip on it makes the track. */
const SpareHeader = memo(function SpareHeader({
  row,
  top,
  height,
}: {
  row: number
  top: number
  height: number
}) {
  const hint = useHint(
    message("No track here yet. Place a clip on this row and track {row} is added for it", { row: plainNumber(row + 1) })
  )
  return (
    <ContextActions items={SPARE_MENU}>
      <div
        data-spare-row={row}
        style={{ top, height }}
        className="absolute inset-x-0 flex items-center border-t border-(--wf-grid-line) bg-(--wf-grid-line)/60 pl-1"
        {...hint}
      >
        <span
          aria-hidden
          className="w-5 shrink-0 text-right font-readout text-[0.625rem] text-muted-foreground/60"
        >
          {row + 1}
        </span>
      </div>
    </ContextActions>
  )
})

/** The corner above the headers and left of the ruler. */
export function TrackCorner() {
  return (
    <div className="flex items-center border-r border-b bg-chassis/30 pr-0.5 pl-2 text-muted-foreground">
      <span className="flex-1 text-[0.6875rem] font-medium">{t("Tracks")}</span>
      <ActionButton
        action="playlist.addTrack"
        variant="ghost"
        size="icon-xs"
        tooltipSide="right"
      >
        <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
      </ActionButton>
    </div>
  )
}

/**
 * The track names beside the grid. They are ordinary elements, placed where
 * the canvas draws its rows and moved with it: only the rows in view exist,
 * and scrolling shifts their container without rendering anything.
 */
export function TrackHeaders({ metrics }: { metrics: GridMetrics }) {
  useEffect(syncTrackHeights, [])
  const { rows: trackRows } = useTrackRows()
  usePlaylistStore((state) => state.trackHeights)
  const target = usePlaylistStore((state) => state.targetTrack)
  const first = useViewportValue(metrics, (viewport) =>
    Math.floor(playlistYToRow(viewport, 0))
  )
  const end = useViewportValue(
    metrics,
    (viewport) => Math.ceil(playlistYToRow(viewport, viewport.height)) + 1
  )
  const rowHeight = useViewportValue(metrics, (viewport) => viewport.rowHeight)
  const dpr = useViewportValue(metrics, (viewport) => viewport.dpr)
  const rowCount = useViewportValue(metrics, (_, limits) => limits.rowCount)
  const rootRef = useRef<HTMLDivElement>(null)
  const scrolledRef = useRef<HTMLDivElement>(null)
  const [reorder, setReorder] = useState<Reorder | null>(null)
  const grab = useRef<Grab | null>(null)
  const lifted = useRef<Reorder | null>(null)

  // A press on a header that then moves up or down carries the track to
  // the gap between two others. The lamp and the name keep their clicks.
  const onGrab = useCallback(
    (event: ReactPointerEvent<HTMLElement>, row: number) => {
      const pressed = event.target
      if (
        event.button !== 0 ||
        (pressed instanceof Element && pressed.closest("button, input"))
      ) {
        return
      }
      grab.current = { row, y: event.clientY, pointer: event.pointerId }
    },
    []
  )

  useEffect(() => {
    const root = rootRef.current
    if (!root) return
    const gapAt = (clientY: number) => {
      const y = logicalDelta(clientY - root.getBoundingClientRect().top)
      return Math.round(playlistYToRow(metrics.viewport, y))
    }
    const onMove = (event: PointerEvent) => {
      const held = grab.current
      if (!held || event.pointerId !== held.pointer) return
      const count = trackRowsNow().rows.length
      if (
        !root.hasPointerCapture(held.pointer) &&
        Math.abs(event.clientY - held.y) < DRAG_THRESHOLD_PX
      ) {
        return
      }
      root.setPointerCapture(held.pointer)
      const gap = Math.min(count, Math.max(0, gapAt(event.clientY)))
      if (lifted.current?.gap === gap) return
      lifted.current = { row: held.row, gap }
      setReorder(lifted.current)
    }
    const finish = (event: PointerEvent, drop: boolean) => {
      const held = grab.current
      if (!held || event.pointerId !== held.pointer) return
      grab.current = null
      if (root.hasPointerCapture(held.pointer)) {
        root.releasePointerCapture(held.pointer)
      }
      const current = lifted.current
      lifted.current = null
      setReorder(null)
      if (!drop || !current) return
      const destination = trackDrop(current.row, current.gap)
      if (destination) void moveTrack(destination.track, destination.index)
    }
    const onUp = (event: PointerEvent) => finish(event, true)
    const onCancel = (event: PointerEvent) => finish(event, false)
    root.addEventListener("pointermove", onMove)
    root.addEventListener("pointerup", onUp)
    root.addEventListener("pointercancel", onCancel)
    return () => {
      root.removeEventListener("pointermove", onMove)
      root.removeEventListener("pointerup", onUp)
      root.removeEventListener("pointercancel", onCancel)
    }
  }, [metrics])

  useEffect(() => {
    const place = () => {
      const scrolled = scrolledRef.current
      if (!scrolled) return
      const { scrollRow, rowHeight, dpr } = metrics.viewport
      const offset = Math.round(scrollRow * rowHeight * dpr) / dpr
      scrolled.style.transform = `translateY(${-offset}px)`
    }
    place()
    return metrics.subscribe(place)
  }, [metrics])

  useEffect(() => {
    const root = rootRef.current
    if (!root) return
    const onWheel = (event: WheelEvent) => {
      event.preventDefault()
      const bounds = root.getBoundingClientRect()
      metrics.wheel(
        wheelInput(event, { x: 0, y: logicalDelta(event.clientY - bounds.top) })
      )
    }
    root.addEventListener("wheel", onWheel, { passive: false })
    return () => root.removeEventListener("wheel", onWheel)
  }, [metrics])

  const rows: React.ReactNode[] = []
  const last = Math.min(rowCount, end)
  for (let row = first; row < last; row++) {
    const entry = trackRows[row]
    const geometry = rowGeometry({ rowHeight })
    const top = Math.round(geometry.top(row) * dpr) / dpr
    const box = {
      top,
      height: Math.round(geometry.top(row + 1) * dpr) / dpr - top,
    }
    rows.push(
      entry?.kind === "group" ? (
        <GroupHeader
          key={`group-${entry.group.id}`}
          group={entry.group}
          members={entry.members}
          depth={entry.depth}
          top={box.top}
          height={box.height}
        />
      ) : entry?.kind === "track" ? (
        <TrackHeader
          key={entry.track.id}
          track={entry.track}
          row={row}
          index={entry.index}
          depth={entry.depth}
          top={box.top}
          height={box.height}
          target={entry.track.id === target}
          lifted={reorder?.row === row}
          onGrab={onGrab}
        />
      ) : (
        <SpareHeader
          key={`spare-${row}`}
          row={row}
          top={box.top}
          height={box.height}
        />
      )
    )
  }

  return (
    <div
      ref={rootRef}
      role="group"
      aria-label={t("Tracks")}
      className="relative overflow-hidden border-r bg-chassis/20"
      style={{ width: HEADER_WIDTH }}
    >
      <div ref={scrolledRef} className="absolute inset-x-0 top-0">
        {rows}
        {reorder !== null && trackDrop(reorder.row, reorder.gap) !== null && (
          <div
            aria-hidden
            data-slot="track-drop-line"
            className="pointer-events-none absolute inset-x-0 z-10 h-0.5 -translate-y-px bg-brand"
            style={{
              top:
                Math.round(rowGeometry({ rowHeight }).top(reorder.gap) * dpr) /
                dpr,
            }}
          />
        )}
      </div>
    </div>
  )
}
