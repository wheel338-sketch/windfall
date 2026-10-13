import { Add01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { useCallback, useEffect, useRef, useState } from "react"

import type { TrackId } from "@/bindings"
import { ActionButton } from "@/components/action-button"
import { ContextActions } from "@/components/context-actions"
import { Empty, EmptyDescription } from "@/components/ui/empty"
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable"
import { useShortcutScope } from "@/lib/actions"
import { useProjectStore, useUiStore } from "@/lib/store"
import type { PanelSizes } from "@/lib/store/ui"
import { MASTER_TRACK } from "@/lib/units"

import { registerMixerActions } from "./actions"
import { useEffectsUi } from "./effects-ui"
import { EffectInspector, EffectInspectorTab } from "./inspector"
import {
  INSPECTOR_MIN_WIDTH,
  inspectorWidthFor,
  MASTER_WIDTH,
  STRIP_WIDTH,
  chosenStripLayout,
} from "./layout"
import { MIXER_MENU } from "./menus"
import { useMixerUi } from "./mixer-ui"
import { watchPeaks } from "./peaks"
import { maxSendCount } from "./routing"
import { MixerStrip } from "./strip"
import { CreateCurrentUtility } from "./current-source"
import { maxEffectCount } from "./strip-track"
import { DockStrips } from "./dock-strips"
import { MixerToolbar } from "./toolbar"
import { matchingTrackIds } from "./track-filter"
import { translatedText as t } from "@/lib/i18n"

/** Where the split between the strips and the effects is remembered. */
const LAYOUT_KEY = "mixer:strips+effects"

/** The mixer track the channel selected in the rack plays into. */
function useLinkedTrack(): TrackId | null {
  const channel = useUiStore((state) => state.selectedChannel)
  return useProjectStore(
    (state) =>
      state.project.channels.find((item) => item.id === channel)?.mixerTrack ??
      null
  )
}

function AddTrack({
  first,
  noMatches,
}: {
  first: boolean
  noMatches: boolean
}) {
  return (
    <div className="flex h-full items-start gap-3 p-1.5">
      <ActionButton
        action="mixer.addTrack"
        variant="outline"
        size="icon-sm"
        tooltipSide="top"
        className="border-dashed text-muted-foreground hover:text-foreground"
      >
        <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
      </ActionButton>
      {noMatches && (
        <Empty className="max-w-64 p-0 pt-0.5">
          <EmptyDescription role="status">{t("No tracks match.")}</EmptyDescription>
        </Empty>
      )}
      {first && (
        <p className="max-w-64 pt-0.5 leading-snug text-muted-foreground">
          {t("No insert tracks yet. Every channel you add to the rack gets its own track here, or add an empty one to use as a bus.")}
        </p>
      )}
    </div>
  )
}

/**
 * The mixer: Master and Current pinned on the left, independent insert
 * docks, and the selected track's effects on the right.
 *
 * Only the strips in view are mounted. A strip is a fader, a knob, a canvas
 * and a dozen store subscriptions, and a full mixer has 500 inserts; mounting
 * the few on screen keeps resizing, scrolling and metering at the cost of
 * what is visible, however long the mixer gets. Peaks and clips of tracks
 * that are out of view are still held (see `peaks.ts`).
 */
export default function MixerPanel() {
  const tracks = useProjectStore((state) => state.project.mixer.tracks)
  const layoutChoice = useUiStore((state) => state.mixerLayout)
  const [height, setHeight] = useState(0)
  const [scrollbars, setScrollbars] = useState({ left: 0, middle: 0, right: 0 })
  const leftScrollbar = useCallback(
    (left: number) =>
      setScrollbars((prior) =>
        prior.left === left ? prior : { ...prior, left }
      ),
    []
  )
  const middleScrollbar = useCallback(
    (middle: number) =>
      setScrollbars((prior) =>
        prior.middle === middle ? prior : { ...prior, middle }
      ),
    []
  )
  const rightScrollbar = useCallback(
    (right: number) =>
      setScrollbars((prior) =>
        prior.right === right ? prior : { ...prior, right }
      ),
    []
  )
  const [filter, setFilter] = useState("")
  const current = useProjectStore(
    (state) => state.project.mixer.tracks.find((track) => track.current)?.id
  )
  const selected = useUiStore((state) => state.selectedTrack)
  const linked = useLinkedTrack()
  const maxSends = useProjectStore((state) =>
    maxSendCount(state.project.mixer.tracks)
  )
  const maxEffects = useProjectStore((state) =>
    maxEffectCount(state.project.mixer.tracks)
  )
  const enlarged = useUiStore((state) => state.centerOverlay === "effects")
  // Enlarged, the effects are in the editor area and not in here.
  const inspectorOpen =
    useEffectsUi((state) => state.inspectorOpen) && !enlarged
  // Once the divider has been dragged, the width it was left at is used.
  const [inspectorWidth] = useState(() => inspectorWidthFor(window.innerWidth))
  const savedLayout = useUiStore((state) => state.layouts[LAYOUT_KEY])
  const saveLayout = useUiStore((state) => state.saveLayout)
  const root = useRef<HTMLDivElement>(null)
  const focusing = useMixerUi((state) => state.focusing)
  const scope = useShortcutScope("mixer")

  useEffect(() => registerMixerActions(), [])
  useEffect(() => watchPeaks(), [])

  const inserts = tracks.filter(
    (track) => track.id !== MASTER_TRACK && !track.current
  )
  const matchingIds = new Set(matchingTrackIds(inserts, filter))
  const visibleInserts = inserts.filter((track) => matchingIds.has(track.id))
  const noMatches = filter.trim().length > 0 && visibleInserts.length === 0
  const scrollbarSpace = Math.max(
    scrollbars.middle,
    visibleInserts.some((track) => track.dock === "left") ? scrollbars.left : 0,
    visibleInserts.some((track) => track.dock === "right")
      ? scrollbars.right
      : 0
  )
  const layout = chosenStripLayout(
    layoutChoice,
    height > 0 ? height - (scrollbarSpace - scrollbars.middle) : height,
    maxSends,
    maxEffects
  )
  const { mode, sendRows, effectRows, width } = layout
  const dockIds = (dock: "left" | "middle" | "right") =>
    visibleInserts
      .filter((track) => (track.dock ?? "middle") === dock)
      .map((track) => track.id)

  // A key that moves the selection takes the focus along. The strip may
  // only just have been mounted, so this waits for it.
  useEffect(() => {
    if (focusing === null) return
    const strip = root.current?.querySelector<HTMLElement>(
      `[data-track="${focusing}"]`
    )
    if (!strip) return
    strip.focus({ preventScroll: true })
    useMixerUi.setState({ focusing: null })
  }, [focusing, selected])

  return (
    <ContextActions items={MIXER_MENU}>
      <div
        ref={root}
        data-slot="mixer"
        data-mode={mode}
        className="relative flex h-full min-h-0 min-w-0 pt-9"
        {...scope}
      >
        <MixerToolbar filter={filter} onFilterChange={setFilter} />
        <ResizablePanelGroup
          id="mixer"
          orientation="horizontal"
          className="min-w-0 flex-1"
          defaultLayout={inspectorOpen ? savedLayout : undefined}
          onLayoutChanged={(sizes: PanelSizes) => {
            if (inspectorOpen) saveLayout(LAYOUT_KEY, sizes)
          }}
        >
          <ResizablePanel
            id="strips"
            minSize={MASTER_WIDTH + 2 * STRIP_WIDTH}
            className="flex min-w-0"
          >
            <div
              data-slot="mixer-master"
              className="z-10 shrink-0 border-r bg-chassis shadow-[2px_0_6px_-2px_rgb(0_0_0/0.35)]"
              style={{ width: MASTER_WIDTH, paddingBottom: scrollbarSpace }}
            >
              <MixerStrip
                id={MASTER_TRACK}
                mode={mode}
                sendRows={sendRows}
                effectRows={effectRows}
                linked={linked === MASTER_TRACK}
                metering
              />
            </div>
            {current !== undefined ? (
              <div
                className="z-10 shrink-0 border-r bg-chassis"
                style={{ width, paddingBottom: scrollbarSpace }}
              >
                <MixerStrip
                  id={current}
                  mode={mode}
                  sendRows={sendRows}
                  effectRows={effectRows}
                  linked={false}
                  metering
                />
              </div>
            ) : (
              <div className="shrink-0 p-1">
                <CreateCurrentUtility />
              </div>
            )}
            {dockIds("left").length > 0 && (
              <DockStrips
                dock="left"
                scrollbarSpace={scrollbarSpace}
                onScrollbar={leftScrollbar}
                ids={dockIds("left")}
                selected={selected}
                linked={linked}
                layout={layout}
              />
            )}
            <DockStrips
              dock="middle"
              scrollbarSpace={scrollbarSpace}
              onScrollbar={middleScrollbar}
              ids={dockIds("middle")}
              selected={selected}
              linked={linked}
              layout={layout}
              onHeight={setHeight}
            >
              <AddTrack
                first={inserts.length === 0 && !noMatches}
                noMatches={noMatches}
              />
            </DockStrips>
            {dockIds("right").length > 0 && (
              <DockStrips
                dock="right"
                scrollbarSpace={scrollbarSpace}
                onScrollbar={rightScrollbar}
                ids={dockIds("right")}
                selected={selected}
                linked={linked}
                layout={layout}
              />
            )}
          </ResizablePanel>
          {inspectorOpen && (
            <>
              <ResizableHandle aria-label={t("Resize the effects")} />
              <ResizablePanel
                id="effects"
                defaultSize={inspectorWidth}
                minSize={INSPECTOR_MIN_WIDTH}
                maxSize="70%"
                groupResizeBehavior="preserve-pixel-size"
              >
                <EffectInspector />
              </ResizablePanel>
            </>
          )}
        </ResizablePanelGroup>
        {!inspectorOpen && <EffectInspectorTab enlarged={enlarged} />}
      </div>
    </ContextActions>
  )
}
