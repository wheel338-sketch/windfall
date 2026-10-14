import { PluginPicker } from "@/features/plugins/picker"
import { PresetRenderDialog } from "@/features/preset-render/dialog"
import type { CSSProperties } from "react"

import { Toaster } from "@/components/ui/sonner"
import { ValueContextMenus } from "@/components/value-context-menu"
import { TooltipProvider } from "@/components/ui/tooltip"
import { RetainedSoundsDialog } from "@/features/flp-import/retained-dialog"
import { FlpImportDialog } from "@/features/flp-import/import-dialog"
import { ExportDialog } from "@/features/export/export-dialog"
import { GettingStartedDialog } from "@/features/help/getting-started-dialog"
import { LfoDialog } from "@/features/automation/lfo-dialog"
import { EventEditorPanel } from "@/features/event-editor/panel"
import { PatternTimelineDialog } from "@/features/piano-roll/pattern-timeline"
import { MidiDialogs } from "@/features/midi/dialogs"
import { CommandPalette } from "@/features/palette/command-palette"
import { SettingsDialog } from "@/features/settings/settings-dialog"
import { StageViews } from "@/features/stage/views"
import { SpectrumPanel } from "@/features/spectrum/panel"
import { ProjectOverviewPanel } from "@/features/project-overview/panel"
import { PhaseMeterPanel } from "@/features/phase-meter/panel"
import { SpectrogramPanel } from "@/features/spectrogram/panel"
import { PerformancePanel } from "@/features/performance/grid"
import { VisualizerPanel } from "@/features/visualizer/panel"
import { VideoPanel } from "@/features/video/panel"
import { ScopePanel } from "@/features/analyzers/scope-panel"
import { AnalyzerSmokeHost } from "@/features/analyzers/smoke-host"
import { TransportBar } from "@/features/transport/transport-bar"
import { resolveTheme, useUiStore } from "@/lib/store/ui"
import { cn } from "@/lib/utils"
import { LanguageBoundary } from "@/lib/i18n/boundary"

import { PanelFrame } from "./panel-frame"
import { PANELS, type PanelId } from "./panels"
import { PromptHost } from "./prompt-host"
import { ArchiveReport } from "./archive-report"
import { StatusBar } from "./status-bar"
import { TitleBar } from "./title-bar"
import { useToastOffset } from "./toast-place"
import { useAppBoot, useWindowTitle } from "./use-app-boot"
import { Workspace } from "./workspace"
import { PluginManager } from "@/features/plugins/manager"
import { TouchControllersWindow } from "@/features/touch-controllers/window"
import { AssistantPanel } from "@/features/assistant/panel"
import { McpPanelHost } from "@/features/mcp/panel"
import { ScriptEditor } from "@/features/assistant/script-editor"
import { RecordingDialog } from "@/features/transport/recording-dialog"
import { useDetachedScope } from "./detached-scope"

/** The most room a toast takes across, however long its message. */
const TOAST_WIDTH = 340

/**
 * A toast is one line, with what it says and the detail side by side, so
 * a stack of them stays low. A long message wraps inside the toast's
 * width, also where it is one long word such as a file's path, and makes
 * the toast taller. It can be closed at its right.
 */
const TOAST_CLASSES = {
  toast:
    "cn-toast min-h-6! gap-1.5! rounded-md! py-0.5! pr-7! pl-2! text-xs! shadow-md!",
  content: "min-w-0! flex-row! flex-wrap! items-baseline! gap-x-2! gap-y-0!",
  // Class lists, passed through cn so they are not mistaken for captions.
  title: cn("min-w-0! leading-5! wrap-anywhere!"),
  description: cn("min-w-0! leading-5! wrap-anywhere! text-muted-foreground!"),
  closeButton:
    "top-1/2! right-1! left-auto! size-4! -translate-y-1/2! transform-none! border-0! bg-transparent! text-muted-foreground! hover:text-foreground!",
}

/** Dialogs and toasts every window needs, whatever it shows. */
export function Overlays() {
  const theme = useUiStore((state) => resolveTheme(state.theme))
  // At the bottom right above the status bar, left of the mixer's effects.
  const offset = useToastOffset()
  return (
    <>
      <LanguageBoundary>
      <PromptHost />
      <PresetRenderDialog />
      <ArchiveReport />
      <PluginManager />
      <PluginPicker />
      <EventEditorPanel />
      <LfoDialog />
      <PatternTimelineDialog />
      <StageViews />
      <SpectrumPanel />
      <ProjectOverviewPanel />
      <PhaseMeterPanel />
      <SpectrogramPanel />
      <ScopePanel />
      <PerformancePanel />
      <VisualizerPanel />
      <VideoPanel />
      <TouchControllersWindow />
      <AssistantPanel />
      <ScriptEditor />
      <GettingStartedDialog />
      </LanguageBoundary>
      <Toaster
        theme={theme}
        position="bottom-right"
        offset={offset}
        mobileOffset={offset}
        visibleToasts={3}
        gap={4}
        closeButton
        style={{ "--width": `${TOAST_WIDTH}px` } as CSSProperties}
        toastOptions={{ classNames: TOAST_CLASSES }}
      />
    </>
  )
}

/** The main window: menus, transport, docked panels and the status bar. */
export function AppShell() {
  useAppBoot({ guardClose: true })
  useWindowTitle()

  return (
    <TooltipProvider delay={500}>
      <ValueContextMenus>
        {/* What translates while it renders is mounted again on a language
            change. The palette, the settings, a MIDI review and the toasts
            follow the language themselves, and keep what they hold. */}
        <LanguageBoundary>
          <div className="flex h-full flex-col bg-chassis">
            <TitleBar />
            <TransportBar />
            <Workspace />
            <StatusBar />
          </div>
          <ExportDialog />
          <FlpImportDialog />
          <RetainedSoundsDialog />
          <AnalyzerSmokeHost />
          <McpPanelHost />
        </LanguageBoundary>
        <CommandPalette />
        <SettingsDialog />
        <MidiDialogs />
        <Overlays />
      </ValueContextMenus>
    </TooltipProvider>
  )
}

/**
 * One panel filling a window of its own, opened with `?view=panel&id=mixer`.
 * Panels read everything from the stores, so this is all a detached window
 * needs.
 */
export function PanelWindow({ panel }: { panel: PanelId }) {
  useAppBoot({ guardClose: false })
  useWindowTitle(PANELS[panel].title)
  useDetachedScope(panel)
  const { title, component: Panel } = PANELS[panel]

  return (
    <TooltipProvider delay={500}>
      <ValueContextMenus>
        <LanguageBoundary>
          <div className="h-full">
            <PanelFrame title={title} panel={panel} detached>
              <Panel />
            </PanelFrame>
          </div>
          <ExportDialog />
          <RecordingDialog />
          <FlpImportDialog />
          <RetainedSoundsDialog />
        </LanguageBoundary>
        <CommandPalette />
        <SettingsDialog />
        <MidiDialogs />
        <Overlays />
      </ValueContextMenus>
    </TooltipProvider>
  )
}
