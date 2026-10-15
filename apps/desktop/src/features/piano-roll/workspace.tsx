import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react"
import { useShallow } from "zustand/react/shallow"

import type {
  ChannelId,
  Lane,
  Note,
  PatternId,
  TimeSignature,
  Timeline,
} from "@/bindings"
import { noteName, type PianoKeyboardHandle } from "@/components/audio"
import { ContextActions } from "@/components/context-actions"
import {
  queryRect,
  rgbFromInt,
  rowToKey,
  velocityPalette,
  visibleTicks,
  type TimeGridView,
  type TimeGridViewOptions,
} from "@/lib/canvas"
import { TimeGridCanvas } from "@/lib/canvas/react"
import { useShortcutScope } from "@/lib/actions"
import { errorMessage } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { usePlayhead } from "@/lib/store/realtime"
import { useChannel, useLane, usePattern } from "@/lib/store/selectors"
import { useTransportStore } from "@/lib/store/transport"
import { ticksPerBar, ticksPerBeat } from "@/lib/time"
import { meterSegments, musicalPosition } from "@/lib/timeline"
import { colorToCss, MAX_PATTERN_STEPS, TICKS_PER_STEP } from "@/lib/units"

import { SessionContext } from "./context"
import { noteEnd } from "./edit-math"
import { createSession, readContext } from "./create-session"
import { closePatternTimeline, patternMeterAt } from "./pattern-timeline"
import { useSampleInfo } from "@/features/channel-rack/inspector/sample-info"
import { buildPianoUnderlay, helperDurationTicks, useWaveformHelper } from "./waveform-helper"
import { attachGridInput } from "./grid-input"
import { showHint } from "./hint"
import { hintFor } from "./intents"
import { KeyGutter } from "./key-gutter"
import { KeyLights } from "./key-lights"
import { LaneHeader, LaneResizer } from "./lane-header"
import { NOTE_MENU, PANEL_MENU } from "./menu"
import { NoteToolsDialog } from "./note-tools-dialog"
import { RiffGeneratorDialog } from "./riff-generator-dialog"
import { SheetExportDialog } from "./score/export-dialog"
import { closeSheetExport } from "./sheet-export"
import { closeRiffGenerator } from "./riff-generator"
import { ProgressionGeneratorDialog } from "./progression-dialog"
import { closeProgressionGenerator } from "./progression"
import { closeNoteLfo } from "@/features/automation/lfo-dialog"
import { closeNoteProperties, NotePropertiesDialog } from "./note-properties"
import { closeNoteCurves, NoteCurvesDialog } from "./note-curves"
import { closeNoteTools } from "./note-tools"
import {
  duplicateOutlinePainter,
  noteLabelPainter,
  patternEndPainter,
} from "./overlays"
import { Ruler } from "./ruler"
import { Scrollbar } from "./scrollbar"
import { setCurrentSession } from "./session"
import { gridSpecFor, snapTicks } from "./snap"
import { scaleRows } from "./scales"
import { usePianoRollStore } from "./store"
import { PianoRollToolbar } from "./toolbar"
import { attachTypingKeyboard } from "./typing-keyboard"
import { attachStepEntry } from "./step-entry"
import { ValueLane } from "./value-lane"
import {
  contentTicks,
  DEFAULT_PX_PER_TICK,
  DEFAULT_ROW_HEIGHT,
  MAX_PX_PER_TICK,
  MAX_ROW_HEIGHT,
  MIN_PX_PER_TICK,
  MIN_ROW_HEIGHT,
  patternTickInSong,
  ROW_COUNT,
} from "./view-math"
import { message, translatedText as t } from "@/lib/i18n"

const GUTTER_WIDTH = 68
const RULER_HEIGHT = 38
const SCROLLBAR_SIZE = 11
const EMPTY_NOTES: readonly Note[] = []
const NO_LANES: readonly Lane[] = []

/** `3.2.120`: bar and beat from 1, then ticks into the beat. */
function formatPosition(tick: number, signature: TimeSignature, timeline?: Timeline): string {
  const position = musicalPosition(Math.round(tick), signature, timeline?.meters)
  return `${position.bar}.${position.beat}.${String(position.tick).padStart(3, "0")}`
}

type WorkspaceProps = {
  patternId: PatternId
  channelId: ChannelId
}

/**
 * The piano roll for one channel of one pattern: toolbar, ruler, keyboard,
 * note grid, value lane and scrollbars around one editor. Project data
 * comes from the store; everything that moves every frame goes straight to
 * the canvases through the session.
 */
export function Workspace({ patternId, channelId }: WorkspaceProps) {
  const pattern = usePattern(patternId)
  const lane = useLane(patternId, channelId)
  const channel = useChannel(channelId)
  const inheritedSignature = useProjectStore(
    (state) => state.project.settings.timeSignature
  )
  const signature = pattern?.timeSignature ?? inheritedSignature
  const timeline = pattern?.timeline
  const noteCurves = pattern?.noteCurves
  const snapId = usePianoRollStore((state) => state.snap)
  const scaleRoot = usePianoRollStore((state) => state.scaleRoot)
  const scaleId = usePianoRollStore((state) => state.scaleId)
  const highlightScale = usePianoRollStore((state) => state.highlightScale)
  const ghosts = usePianoRollStore((state) => state.ghosts)
  const helper = useWaveformHelper()
  const helperAsset = useProjectStore((state) => state.project.samples.find((sample) => sample.id === helper.sample))
  const helperState = useSampleInfo(helper.visible || helper.open ? helperAsset : undefined)
  const helperInfo = helperState?.status === "ready" ? helperState.info : null
  const tempo = useProjectStore((state) => state.project.settings.tempoBpm)
  const laneKind = usePianoRollStore((state) => state.laneKind)
  const laneHeight = usePianoRollStore((state) => state.laneHeight)
  const ghostLanes = useProjectStore(
    useShallow((state) => {
      if (!ghosts) return NO_LANES
      const lanes = state.project.patterns.find(
        (item) => item.id === patternId
      )?.lanes
      return lanes?.filter((item) => item.channel !== channelId) ?? NO_LANES
    })
  )

  const lengthSteps = pattern?.lengthSteps ?? 0
  const color = channel?.color ?? 0x888888
  const laneKey = `${patternId}:${channelId}`

  const scope = useShortcutScope("pianoRoll")
  const rootRef = useRef<HTMLDivElement>(null)
  const gridRef = useRef<HTMLDivElement>(null)
  const readoutRef = useRef<HTMLOutputElement>(null)
  const wasPlaying = useRef(false)

  const [session] = useState(createSession)
  const [lights] = useState(() => new KeyLights())
  const attachKeyboard = useCallback(
    (keyboard: PianoKeyboardHandle | null) =>
      lights.attach(keyboard ? (key, lit) => keyboard.setLit(key, lit) : null),
    [lights]
  )
  const [view, setView] = useState<TimeGridView | null>(null)
  const [failure, setFailure] = useState<string | null>(null)

  useLayoutEffect(() => {
    const root = rootRef.current
    if (!root) return
    const stopStepEntry = attachStepEntry(root, channelId)
    const stopTyping = attachTypingKeyboard(root, channelId)
    return () => {
      stopStepEntry()
      stopTyping()
    }
  }, [channelId])

  const lastEnd = useMemo(
    () =>
      (lane?.notes ?? EMPTY_NOTES).reduce(
        (end, note) => Math.max(end, noteEnd(note)),
        0
      ),
    [lane]
  )
  const helperEnd = helper.visible && helperInfo
    ? Math.min(MAX_PATTERN_STEPS * TICKS_PER_STEP, Math.max(0, helper.start + helperDurationTicks(helper, helperInfo, tempo, lengthSteps * TICKS_PER_STEP))) : 0
  const scrollable = contentTicks(lengthSteps, Math.max(lastEnd, helperEnd), ticksPerBar(signature))

  const [options] = useState<TimeGridViewOptions>(() => ({
    renderer: "auto",
    limits: {
      rowCount: ROW_COUNT,
      contentTicks: scrollable,
      minPxPerTick: MIN_PX_PER_TICK,
      maxPxPerTick: MAX_PX_PER_TICK,
      minRowHeight: MIN_ROW_HEIGHT,
      maxRowHeight: MAX_ROW_HEIGHT,
    },
    initial: { pxPerTick: DEFAULT_PX_PER_TICK, rowHeight: DEFAULT_ROW_HEIGHT },
  }))
  const onReady = useCallback((created: TimeGridView) => setView(created), [])
  const onError = useCallback(
    (error: unknown) => setFailure(errorMessage(error)),
    []
  )

  // The editor follows the project before the browser paints.
  useLayoutEffect(() => {
    session.setEditing(patternId, channelId)
    session.editor.setContext(readContext(patternId, channelId))
    session.notifyView()
    session.view?.invalidate("overlay")
  }, [session, patternId, channelId, lane, lengthSteps, signature, timeline, noteCurves])

  useEffect(() => {
    setCurrentSession(session)
    return () => {
      setCurrentSession(null)
      closeNoteTools()
      closeRiffGenerator()
      closeSheetExport()
      closeProgressionGenerator()
      closeNoteProperties()
      closeNoteCurves()
      closeNoteLfo()
      closePatternTimeline()
      showHint(null)
      session.editor.dispose()
      session.dispose()
    }
  }, [session])

  // Shortcuts go to the notes as soon as the panel opens, unless the
  // keyboard focus is somewhere on purpose.
  useEffect(() => {
    session.setFocusTarget(() => gridRef.current)
    const frame = requestAnimationFrame(() => {
      const focused = document.activeElement
      if (focused === null || focused === document.body) session.focusGrid()
    })
    return () => cancelAnimationFrame(frame)
  }, [session])

  useEffect(() => {
    if (!view) return
    session.attachView(view)
    const { editor } = session
    const stop = [
      attachGridInput(session, view),
      view.addOverlayPainter(noteLabelPainter(session)),
      view.addOverlayPainter(duplicateOutlinePainter(session)),
      view.addOverlayPainter(patternEndPainter(session)),
      // Labels and outlines follow the notes, which the overlay cannot see.
      editor.subscribe((event) => {
        if (event !== "hover") view.invalidate("overlay")
      }),
    ]
    return () => {
      for (const off of stop) off()
      session.attachView(null)
    }
  }, [session, view])

  useEffect(() => {
    if (!view) return
    view.setLimits({ contentTicks: scrollable })
    session.notifyView()
  }, [session, view, scrollable])

  useEffect(() => {
    if (view) session.showLane(laneKey)
  }, [session, view, laneKey])

  // Row shading is shared by Canvas 2D, WebGL2 and WebGPU.
  useEffect(() => {
    view?.setRows(
      scaleRows(highlightScale ? { root: scaleRoot, id: scaleId } : null)
    )
  }, [view, scaleRoot, scaleId, highlightScale])

  // The finest grid lines follow the snap for as long as the zoom shows them.
  useEffect(() => {
    if (!view) return
    const apply = () => {
      const px = view.viewport.pxPerTick
      const base = gridSpecFor(snapTicks(snapId, signature), px, signature)
      view.setTimeGrid({ ...base, segments: timeline?.meters.length ? meterSegments(signature, timeline.meters).map((segment) => ({ ...gridSpecFor(snapTicks(snapId, segment.signature), px, segment.signature), start: segment.start, end: segment.end })) : undefined })
    }
    apply()
    return view.onViewportChange(apply)
  }, [view, snapId, signature, timeline])

  useEffect(() => {
    if (!view) return
    const apply = () =>
      session.editor.setPalette(velocityPalette(view.theme, rgbFromInt(color)))
    apply()
    return view.onThemeChange(apply)
  }, [session, view, color])

  useEffect(() => {
    if (!view) return
    const apply = () =>
      view.setUnderlay(buildPianoUnderlay(ghostLanes, view.theme, helper, helperInfo, tempo, lengthSteps * TICKS_PER_STEP))
    apply()
    return view.onThemeChange(apply)
  }, [view, ghostLanes, helper, helperInfo, tempo, lengthSteps])

  // What is under the pointer and what is selected, shown around the grid.
  useEffect(() => {
    const { editor } = session
    return editor.subscribe((event) => {
      if (event === "hover") {
        const hover = editor.hover
        lights.set("hover", hover ? [hover.key] : [])
        showHint(
          hintFor(hover?.intent ?? null, usePianoRollStore.getState().tool)
        )
        // The last position stays up when the pointer leaves the grid.
        const readout = readoutRef.current
        if (readout && hover) {
          const pattern = editor.context?.pattern
          if (pattern) {
            const position = `${formatPosition(hover.tick, pattern.signature, pattern.timeline)}  ${noteName(hover.key)}`
            // While an edge is dragged, the length it makes is shown too.
            const length = editor.resizedLength
            const beat = ticksPerBeat(patternMeterAt(hover.tick, pattern.signature, pattern.timeline).signature)
            readout.textContent = length === null ? position : message("{position}  {count} beats long", { position, count: Math.round((length / beat) * 1000) / 1000 })
          }
        }
        return
      }
      const lift = editor.drag?.keys ?? 0
      const keys = new Set<number>()
      for (const note of editor.selectedNotes()) keys.add(note.key + lift)
      lights.set("selected", keys)
    })
  }, [session, lights])

  usePlayhead((tick, playing) => {
    const context = session.editor.context
    const transport = useTransportStore.getState()
    let local: number | null = null
    if (context) {
      if (transport.mode === "pattern") {
        const shown = playing || tick > 0
        if (shown && transport.pattern === context.pattern.id) local = tick
      } else if (playing) {
        const { playlist } = useProjectStore.getState().project
        local = patternTickInSong(
          tick,
          playlist.clips,
          playlist.tracks,
          context.pattern.id,
          context.pattern.lengthSteps * TICKS_PER_STEP
        )
      }
    }
    if (!session.editor.scrubbing) session.setPlayhead(local)

    const sounding = playing && local !== null
    if (sounding) {
      const items = session.editor.items
      const keys = queryRect(items, local ?? 0, (local ?? 0) + 1, 0, ROW_COUNT)
      lights.set(
        "playing",
        keys.map((index) => rowToKey(items.batch.row(index)))
      )
    } else if (wasPlaying.current) {
      lights.set("playing", [])
    }
    wasPlaying.current = sounding

    const current = session.view
    if (
      sounding &&
      local !== null &&
      current &&
      usePianoRollStore.getState().follow &&
      !session.editor.busy
    ) {
      // Turn the page when the playhead leaves the view on either side.
      const range = visibleTicks(current.viewport)
      const span = range.end - range.start
      if (local < range.start || local > range.end - span * 0.05) {
        current.setViewport({
          ...current.viewport,
          scrollTick: local - span * 0.05,
        })
      }
    }
  })

  return (
    <SessionContext value={session}>
      <NoteToolsDialog />
      <RiffGeneratorDialog />
      <SheetExportDialog />
      <ProgressionGeneratorDialog />
      <NotePropertiesDialog />
      <NoteCurvesDialog />
      <ContextActions items={PANEL_MENU}>
        <div
          ref={rootRef}
          className="flex h-full min-h-0 min-w-0 flex-col bg-background"
          {...scope}
        >
          <PianoRollToolbar channelId={channelId} readoutRef={readoutRef} />
          <div
            className="grid min-h-0 flex-1"
            style={{
              gridTemplateColumns: `${GUTTER_WIDTH}px minmax(0, 1fr) ${SCROLLBAR_SIZE}px`,
              gridTemplateRows: `${RULER_HEIGHT}px minmax(0, 1fr) auto ${laneHeight}px ${SCROLLBAR_SIZE}px`,
            }}
          >
            <div className="border-r border-b bg-chassis/60" />
            <div className="border-b bg-chassis/60">
              <Ruler />
            </div>
            <div className="border-b border-l bg-chassis/40" />

            <div className="min-h-0 border-r">
              <KeyGutter
                channelId={channelId}
                color={colorToCss(color)}
                keyboardRef={attachKeyboard}
              />
            </div>
            <ContextActions items={NOTE_MENU}>
              <div
                ref={gridRef}
                tabIndex={0}
                role="application"
                aria-label={t("Note grid")}
                className="focus-frame relative min-h-0 min-w-0"
              >
                {failure === null ? (
                  <TimeGridCanvas
                    options={options}
                    onReady={onReady}
                    onError={onError}
                    className="absolute inset-0"
                  />
                ) : (
                  <p
                    role="alert"
                    className="absolute inset-0 flex items-center justify-center p-6 text-center text-muted-foreground"
                  >
                    {t("The note grid could not start:")}{" "}{failure}
                  </p>
                )}
              </div>
            </ContextActions>
            <div className="min-h-0 border-l">
              <Scrollbar axis="rows" />
            </div>

            <div className="col-span-3">
              <LaneResizer />
            </div>

            <div className="border-r bg-chassis/40">
              <LaneHeader />
            </div>
            <div className="min-w-0">
              <ValueLane kind={laneKind} color={color} />
            </div>
            <div className="border-l bg-chassis/40" />

            <div className="border-t border-r bg-chassis/40" />
            <div className="min-w-0 border-t">
              <Scrollbar axis="time" />
            </div>
            <div className="border-t border-l bg-chassis/40" />
          </div>
        </div>
      </ContextActions>
    </SessionContext>
  )
}
