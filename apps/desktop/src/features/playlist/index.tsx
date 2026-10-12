import { useEffect, useMemo } from "react"

import { VideoTimelineLane } from "@/features/video/timeline-lane"
import { ContextActions } from "@/components/context-actions"
import { ProjectInfoControls } from "@/features/project-info"
import { NotebookControls } from "@/features/notebook"
import { useShortcutScope } from "@/lib/actions"
import { useProjectGeneration } from "@/lib/store/replaced"
import { useTransportStore } from "@/lib/store/transport"

import { ArrangementControls } from "./arrangement/controls"
import { setActiveMetrics } from "./active"
import { ClipInspector } from "./audio/clip-inspector"
import { AudioCompDialog } from "./audio/comp-dialog"
import { TakeGroupControls } from "./audio/take-group-controls"
import { PlaylistGrid } from "./grid"
import { HEADER_WIDTH, RULER_HEIGHT, SCROLLBAR_SIZE } from "./layout"
import { PANEL_MENU } from "./menu"
import { GridMetrics } from "./metrics"
import { applySongCursor } from "./ops"
import { PatternPicker } from "./pattern-picker"
import { Ruler } from "./ruler"
import { Scrollbar } from "./scrollbar"
import { usePlaylistStore } from "./store"
import { PlaylistToolbar } from "./toolbar"
import { TimelineControls } from "./timeline-controls"
import { TrackCorner, TrackHeaders } from "./track-headers"

const GRID_TEMPLATE = {
  gridTemplateColumns: `${HEADER_WIDTH}px minmax(0, 1fr) ${SCROLLBAR_SIZE}px`,
  gridTemplateRows: `${RULER_HEIGHT}px minmax(0, 1fr) ${SCROLLBAR_SIZE}px`,
}

/**
 * The playlist: the song's timeline. Patterns made in the channel rack and
 * the piano roll are laid out here as clips on tracks, beside audio clips
 * and automation clips. A toolbar on top, the settings of the selected
 * audio clips under it, what can be placed at the left, then track names,
 * a bar ruler and the clip grid.
 */
export default function PlaylistPanel() {
  // Another project starts the timeline over at its beginning.
  const generation = useProjectGeneration()
  return <Playlist key={generation} />
}

function Playlist() {
  const metrics = useMemo(() => new GridMetrics(), [])
  const pickerOpen = usePlaylistStore((state) => state.pickerOpen)
  const scope = useShortcutScope("playlist")

  useEffect(() => {
    const stops = [
      setActiveMetrics(metrics),
      // However song mode is switched on, the song starts where the ruler
      // was last set.
      useTransportStore.subscribe((state, previous) => {
        if (state.mode === "song" && previous.mode !== "song") {
          void applySongCursor()
        }
        // A pattern picked anywhere in the app is what gets placed next.
        if (state.pattern !== previous.pattern) {
          usePlaylistStore.getState().setBrush({ type: "pattern" })
        }
      }),
    ]
    return () => {
      for (const stop of stops) stop()
    }
  }, [metrics])

  return (
    <ContextActions items={PANEL_MENU}>
      <div
        data-slot="playlist"
        className="flex h-full min-h-0 min-w-0 flex-col"
        {...scope}
      >
        <div className="flex shrink-0 items-center border-b">
          <div className="min-w-0 flex-1">
            <PlaylistToolbar metrics={metrics} />
          </div>
          <ProjectInfoControls />
          <NotebookControls />
        </div>
        <TimelineControls />
        <TakeGroupControls />
        <ArrangementControls />
        <ClipInspector />
        <AudioCompDialog />
        <VideoTimelineLane metrics={metrics} />
        <div className="flex min-h-0 flex-1">
          {pickerOpen && <PatternPicker />}
          <div className="grid min-h-0 min-w-0 flex-1" style={GRID_TEMPLATE}>
            <TrackCorner />
            <Ruler metrics={metrics} />
            <div className="border-b border-l bg-chassis/30" />
            <TrackHeaders metrics={metrics} />
            <PlaylistGrid metrics={metrics} />
            <Scrollbar metrics={metrics} axis="tracks" />
            <div className="border-t border-r bg-chassis/30" />
            <Scrollbar metrics={metrics} axis="time" />
            <div className="border-t border-l bg-chassis/30" />
          </div>
        </div>
      </div>
    </ContextActions>
  )
}
