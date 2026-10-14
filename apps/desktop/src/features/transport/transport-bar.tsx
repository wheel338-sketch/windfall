import { MidiRecordingControls } from "@/features/controllers/recording-controls"
import { useRecordingStore } from "./recording-store"
import { RecordingDialog } from "./recording-dialog"
import { AudioLoggerDialog } from "@/features/audio-logger/logger-dialog"
import {
  Add01Icon,
  PlayIcon,
  Redo02Icon,
  StopIcon,
  Undo02Icon,
} from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { useEffect } from "react"

import { ActionButton } from "@/components/action-button"
import { ContextActions } from "@/components/context-actions"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"
import {
  useAutomationRecordStore,
  watchAutomationRecording,
} from "@/features/automation/record"
import { HistoryPopover } from "@/features/history/history-popover"
import { TRANSPORT_MENU } from "@/features/layout/chrome-menus"
import { useTransportStore } from "@/lib/store/transport"
import { cn } from "@/lib/utils"

import { MasterMeterSlot } from "./master-meter-slot"
import { ModeSwitch } from "./mode-switch"
import { PatternSelector } from "./pattern-selector"
import { PerformanceReadout } from "./performance-readout"
import { PositionReadout } from "./position-readout"
import { TempoField } from "./tempo-field"
import { TimeSignatureField } from "./time-signature"
import { TapTempoDialog } from "./tap-tempo-dialog"
import { MetronomeControls } from "./metronome-controls"
import { translatedText as t } from "@/lib/i18n"

function Divider() {
  return <Separator orientation="vertical" className="mx-1 my-2.5" />
}

function PlayControls() {
  const playing = useTransportStore((state) => state.playing)
  return (
    <div className="flex items-center gap-1">
      <ActionButton
        action="transport.toggle"
        variant={playing ? "default" : "secondary"}
        size="icon-lg"
        aria-pressed={playing}
        className={cn(
          playing && "bg-brand text-brand-foreground hover:bg-brand/85"
        )}
      >
        <HugeiconsIcon
          icon={PlayIcon}
          strokeWidth={2}
          className="fill-current"
        />
      </ActionButton>
      <ActionButton action="transport.stop" variant="secondary" size="icon-lg">
        <HugeiconsIcon
          icon={StopIcon}
          strokeWidth={2}
          className="fill-current"
        />
      </ActionButton>
    </div>
  )
}

/**
 * The readout window: position, tempo and time signature. It stays dark in
 * both themes, like the display on a piece of hardware.
 */
function Display() {
  return (
    <div className="flex h-8 items-center gap-1 rounded-md bg-display pr-1.5 pl-3 shadow-[inset_0_1px_2px_rgb(0_0_0/0.35)] ring-1 ring-foreground/10">
      <PositionReadout />
      <span aria-hidden className="mx-1.5 h-4 w-px bg-display-foreground/15" />
      <TempoField />
      <span className="text-[0.625rem] text-display-dim">bpm</span>
      <span aria-hidden className="mx-1.5 h-4 w-px bg-display-foreground/15" />
      <TimeSignatureField />
    </div>
  )
}

/**
 * Everything that drives playback, in one strip under the menu bar. It reads
 * all of its state from the stores, so it can be mounted anywhere.
 */
export function TransportBar() {
  const recording = useRecordingStore((state) => state.state.active)
  const recordArmed = useAutomationRecordStore((state) => state.recordArmed)
  useEffect(() => watchAutomationRecording(), [])
  return (
    <ContextActions items={TRANSPORT_MENU}>
      <div
        role="toolbar"
        aria-label={t("Transport")}
        className="flex min-h-12 shrink-0 flex-wrap items-center gap-x-2 gap-y-1 overflow-hidden border-b bg-chassis px-2 py-1.5"
      >
        <PlayControls />
        <ActionButton
          action="recording.open"
          variant={recording ? "destructive" : "secondary"}
        >
          {recording ? t("Recording...") : t("Record")}
        </ActionButton>
        <RecordingDialog />
        <MidiRecordingControls />
        <ActionButton action="audioLogger.open" variant="ghost" size="sm">
          {t("Logger")}
        </ActionButton>
        <AudioLoggerDialog />
        <Button
          title={t("Record automation")}
          aria-pressed={recordArmed}
          variant={recordArmed ? "destructive" : "secondary"}
          onClick={() =>
            useAutomationRecordStore.setState((state) => ({
              recordArmed: !state.recordArmed,
            }))
          }
        >
          {t("Record automation")}
        </Button>
        <ModeSwitch />
        <MetronomeControls />
        <Divider />
        <Display />
        <ActionButton action="tempo.tap" variant="ghost" size="sm">
          {t("Tap")}
        </ActionButton>
        <TapTempoDialog />
        <Divider />
        <div className="flex items-center gap-1">
          <PatternSelector />
          <ActionButton action="pattern.add" variant="ghost" size="icon">
            <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
          </ActionButton>
        </div>

        {/* In a window too narrow for one row, the meter and readouts move
            to a second row whole rather than being drawn over the pattern
            selector or squeezed out. */}
        <div className="flex min-w-72 flex-1 items-center justify-end gap-3 overflow-hidden">
          <MasterMeterSlot />
          <PerformanceReadout />
        </div>
        <Divider />
        <div className="flex items-center">
          <ActionButton action="edit.undo" variant="ghost" size="icon">
            <HugeiconsIcon icon={Undo02Icon} strokeWidth={2} />
          </ActionButton>
          <ActionButton action="edit.redo" variant="ghost" size="icon">
            <HugeiconsIcon icon={Redo02Icon} strokeWidth={2} />
          </ActionButton>
          <HistoryPopover />
        </div>
      </div>
    </ContextActions>
  )
}
