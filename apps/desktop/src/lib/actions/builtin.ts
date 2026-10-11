import {
  openRecording,
  startRecording,
  stopRecording,
  cancelRecording,
  useRecordingStore,
} from "@/features/transport/recording-store"
import { openAudioLogger } from "@/features/audio-logger/logger-store"
import { toast } from "sonner"
import { openGettingStarted } from "@/features/help/getting-started-store"
import {
  canArchive,
  portableUnavailable,
  saveNewVersion,
  saveArchive,
  cancelArchive,
  useArchiveStore,
} from "@/lib/flows/portable"

import {
  addChannel,
  addPattern,
  deletePattern,
  duplicatePattern,
  renamePattern,
  stepPattern,
} from "@/lib/flows/edit"
import {
  canScaleTempo,
  resetTempo,
  scaleTempo,
  tapTempo,
} from "@/lib/flows/tempo"
import {
  newProject,
  openProject,
  openProjectPath,
  reloadMissingSamples,
  saveProject,
  saveProjectAs,
} from "@/lib/flows/project"
import type { EngineStatus } from "@/bindings"
import { useEngineStore } from "@/lib/store/engine"
import { redo, undo } from "@/lib/store/project"
import { selectedPatternId } from "@/lib/store/selectors"
import {
  play,
  setPlayMode,
  setMetronome,
  stop,
  togglePlayback,
  useTransportStore,
} from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { fileName, formatSampleRate } from "@/lib/time"
import { DEFAULT_TEMPO_BPM } from "@/lib/units"

import { getAppState } from "."
import { FL_KEYMAP } from "./keymap"
import { linksOfScope } from "./scope"
import {
  isEnabled,
  registry,
  type Action,
  type AppState,
  type EditCommand,
} from "./registry"

const ui = () => useUiStore.getState()

/** The audio output in one line, for a bug report or a forum post. */
export function deviceInfo(status: EngineStatus): string {
  if (!status.running) {
    return `No audio: ${status.error ?? "the output is not running"}`
  }
  const plugins =
    status.latencyFrames > 0 ? `, plus ${status.latencyFrames} samples` : ""
  return `${status.host} ${status.device ?? "default device"}, ${formatSampleRate(status.sampleRate)}, ${status.bufferFrames} samples, ${status.latencyMs.toFixed(1)} ms${plugins}`
}

async function copyDeviceInfo() {
  const status = useEngineStore.getState().status
  if (!status) return
  const text = deviceInfo(status)
  await navigator.clipboard.writeText(text)
  toast.success("Copied", { description: text })
}

const EDIT_COMMANDS: { command: EditCommand; title: string; words: string }[] =
  [
    { command: "cut", title: "Cut", words: "clipboard move" },
    { command: "copy", title: "Copy", words: "clipboard" },
    { command: "paste", title: "Paste", words: "clipboard insert" },
    { command: "duplicate", title: "Duplicate", words: "clone repeat" },
    { command: "delete", title: "Delete", words: "remove erase" },
    { command: "selectAll", title: "Select all", words: "everything" },
  ]

/**
 * What an Edit menu command means right now: the action that declares that
 * command in the scope that has the keyboard, or in the nearest scope
 * around it. An inspector that keeps the command from its panel ends the
 * search, so Edit > Delete is off there instead of deleting the channel.
 * An action that can run is preferred, so the menu item is enabled whenever
 * any of them is.
 */
function editTarget(command: EditCommand, state: AppState): Action | undefined {
  const declared = registry
    .list()
    .filter((action) => action.editCommand === command)
  for (const link of linksOfScope(state.ui.activeScope)) {
    const candidates = declared.filter((action) => action.scope === link.scope)
    if (candidates.length > 0) {
      return (
        candidates.find((action) => isEnabled(action, state)) ?? candidates[0]
      )
    }
    if (link.keeps.includes(command)) return undefined
  }
  return undefined
}

function editAction({ command, title, words }: (typeof EDIT_COMMANDS)[0]) {
  const target = (state: AppState) => editTarget(command, state)
  const action: Action = {
    id: `edit.${command}`,
    title,
    section: "Edit",
    keywords: `${words} selection`,
    standsFor: (state) => target(state)?.id,
    enabled: (state) => {
      const found = target(state)
      return found !== undefined && isEnabled(found, state)
    },
    run: () => target(getAppState())?.run(),
  }
  return action
}

function patternIndex(state: AppState): number {
  const id = selectedPatternId(state.document.project, state.transport.pattern)
  return state.document.project.patterns.findIndex((item) => item.id === id)
}

/** The actions the shell itself provides. Panels register their own. */
export const BUILTIN_ACTIONS: Action[] = [
  {
    id: "file.importMidi",
    title: "Import MIDI…",
    section: "File",
    keywords: "mid append notes arrangement",
    run: () => ui().openDialog("midiImport"),
  },
  {
    id: "file.exportMidi",
    title: "Export MIDI…",
    section: "File",
    keywords: "mid notes tempo pattern song",
    run: () => ui().openDialog("midiExport"),
  },
  {
    id: "recording.open",
    title: "Record audio input",
    section: "Transport",
    run: openRecording,
  },
  {
    id: "audioLogger.open",
    title: "Audio logger…",
    section: "Transport",
    keywords: "background history recover take capture retrospective log",
    run: openAudioLogger,
  },
  {
    id: "recording.start",
    title: "Start recording",
    section: "Transport",
    enabled: () =>
      !useRecordingStore.getState().busy &&
      !useRecordingStore.getState().state.active &&
      useRecordingStore.getState().inputs.length > 0,
    run: startRecording,
  },
  {
    id: "recording.stop",
    title: "Stop recording and keep take",
    section: "Transport",
    enabled: () =>
      !useRecordingStore.getState().busy &&
      useRecordingStore.getState().state.active,
    run: stopRecording,
  },
  {
    id: "recording.cancel",
    title: "Discard recording",
    section: "Transport",
    enabled: () =>
      !useRecordingStore.getState().busy &&
      useRecordingStore.getState().state.active,
    run: cancelRecording,
  },

  {
    id: "file.new",
    title: "New project",
    section: "File",
    defaultShortcut: "Mod+N",
    run: newProject,
  },
  {
    id: "file.open",
    title: "Open project…",
    section: "File",
    defaultShortcut: "Mod+O",
    run: openProject,
  },
  {
    id: "file.importFlp",
    title: "Import FL Studio project…",
    section: "File",
    keywords: "flp convert notes arrangement samples",
    run: () => ui().openDialog("flpImport"),
  },
  {
    id: "file.save",
    title: "Save",
    section: "File",
    defaultShortcut: "Mod+S",
    keywords: "write project",
    run: async () => {
      await saveProject()
    },
  },
  {
    id: "file.saveAs",
    title: "Save as…",
    section: "File",
    defaultShortcut: "Mod+Shift+S",
    run: async () => {
      await saveProjectAs()
    },
  },
  {
    id: "file.saveNewVersion",
    title: "Save new version",
    section: "File",
    keywords: "numbered snapshot incremental",
    enabled: canArchive,
    whyDisabled: portableUnavailable,
    run: async () => {
      await saveNewVersion()
    },
  },
  {
    id: "file.archive",
    title: "Export portable project archive…",
    section: "File",
    keywords: "zip package samples backup share",
    enabled: canArchive,
    whyDisabled: portableUnavailable,
    run: saveArchive,
  },
  {
    id: "file.cancelArchive",
    title: "Cancel project archive",
    section: "File",
    enabled: () => useArchiveStore.getState().cancellable,
    run: cancelArchive,
  },
  {
    id: "file.reloadSamples",
    title: "Reload missing samples",
    section: "File",
    keywords: "retry find relink audio files warnings",
    run: reloadMissingSamples,
  },
  {
    id: "file.export",
    title: "Export audio…",
    section: "File",
    defaultShortcut: "Mod+E",
    keywords: "render bounce wav",
    run: () => ui().openDialog("export"),
  },

  {
    id: "edit.undo",
    title: "Undo",
    section: "Edit",
    defaultShortcut: "Mod+Z",
    repeats: true,
    enabled: (state) => state.document.history.cursor > 0,
    run: undo,
  },
  {
    id: "edit.redo",
    title: "Redo",
    section: "Edit",
    defaultShortcut: ["Mod+Shift+Z", "Mod+Y"],
    repeats: true,
    enabled: (state) =>
      state.document.history.cursor < state.document.history.entries.length,
    run: redo,
  },
  {
    id: "edit.history",
    title: "History",
    section: "Edit",
    keywords: "steps list earlier back",
    run: () => ui().setHistoryOpen(true),
  },
  ...EDIT_COMMANDS.map(editAction),

  {
    id: "transport.toggle",
    title: "Play or stop",
    section: "Transport",
    defaultShortcut: "Space",
    keywords: "start pause playback",
    run: togglePlayback,
  },
  {
    id: "transport.play",
    title: "Play",
    section: "Transport",
    enabled: (state) => !state.transport.playing,
    run: play,
  },
  {
    id: "transport.stop",
    title: "Stop",
    section: "Transport",
    run: stop,
  },
  {
    id: "transport.metronome",
    title: "Metronome",
    section: "Transport",
    keywords: "click beat bar accent time signature",
    checked: (state) => state.transport.metronome?.enabled ?? false,
    enabled: () => !useRecordingStore.getState().state.active,
    run: () => setMetronome({ enabled: !useTransportStore.getState().metronome?.enabled }),
  },
  {
    id: "transport.patternMode",
    title: "Play the pattern",
    section: "Transport",
    keywords: "pattern mode loop",
    checked: (state) => state.transport.mode === "pattern",
    run: () => setPlayMode("pattern"),
  },
  {
    id: "transport.songMode",
    title: "Play the song",
    section: "Transport",
    keywords: "song mode playlist",
    checked: (state) => state.transport.mode === "song",
    run: () => setPlayMode("song"),
  },
  {
    id: "transport.toggleMode",
    title: "Switch between pattern and song",
    section: "Transport",
    defaultShortcut: "L",
    run: () =>
      setPlayMode(
        useTransportStore.getState().mode === "pattern" ? "song" : "pattern"
      ),
  },

  {
    id: "tempo.tap",
    title: "Tap tempo",
    section: "Transport",
    keywords: "bpm beat measure speed",
    run: tapTempo,
  },
  {
    id: "tempo.half",
    title: "Halve the tempo",
    section: "Transport",
    keywords: "bpm half time slower",
    enabled: (state) =>
      canScaleTempo(state.document.project.settings.tempoBpm, 0.5),
    run: () => scaleTempo(0.5),
  },
  {
    id: "tempo.double",
    title: "Double the tempo",
    section: "Transport",
    keywords: "bpm double time faster",
    enabled: (state) =>
      canScaleTempo(state.document.project.settings.tempoBpm, 2),
    run: () => scaleTempo(2),
  },
  {
    id: "tempo.reset",
    title: `Reset the tempo to ${DEFAULT_TEMPO_BPM} BPM`,
    section: "Transport",
    keywords: "bpm default",
    enabled: (state) =>
      state.document.project.settings.tempoBpm !== DEFAULT_TEMPO_BPM,
    run: resetTempo,
  },

  {
    id: "channel.add",
    title: "Add channel",
    section: "Channels",
    defaultShortcut: "Alt+C",
    keywords: "new sampler instrument",
    run: addChannel,
  },

  {
    id: "pattern.add",
    title: "Add pattern",
    section: "Patterns",
    defaultShortcut: "Alt+P",
    keywords: "new",
    run: addPattern,
  },
  {
    id: "pattern.next",
    title: "Next pattern",
    section: "Patterns",
    defaultShortcut: "Mod+ArrowDown",
    enabled: (state) =>
      patternIndex(state) < state.document.project.patterns.length - 1,
    run: () => stepPattern(1),
  },
  {
    id: "pattern.previous",
    title: "Previous pattern",
    section: "Patterns",
    defaultShortcut: "Mod+ArrowUp",
    enabled: (state) => patternIndex(state) > 0,
    run: () => stepPattern(-1),
  },
  {
    id: "pattern.rename",
    title: "Rename pattern…",
    section: "Patterns",
    run: () => renamePattern(),
  },
  {
    id: "pattern.duplicate",
    title: "Duplicate pattern",
    section: "Patterns",
    keywords: "clone copy",
    run: () => duplicatePattern(),
  },
  {
    id: "pattern.delete",
    title: "Delete pattern",
    section: "Patterns",
    keywords: "remove",
    enabled: (state) => state.document.project.patterns.length > 1,
    run: () => deletePattern(),
  },

  {
    id: "view.commandPalette",
    title: "Command palette…",
    section: "View",
    defaultShortcut: "Mod+K",
    keywords: "search actions shortcuts",
    run: () => {
      if (ui().dialog === "palette") ui().closeDialog()
      else ui().openDialog("palette")
    },
  },
  {
    id: "view.browser",
    title: "Browser",
    section: "View",
    defaultShortcut: "Mod+B",
    keywords: "show hide toggle samples files panel",
    checked: (state) => state.ui.panels.browser,
    run: () => ui().togglePanel("browser"),
  },
  {
    id: "view.mixer",
    title: "Mixer",
    section: "View",
    defaultShortcut: "Mod+J",
    keywords: "show hide toggle panel",
    checked: (state) => state.ui.panels.mixer,
    run: () => ui().togglePanel("mixer"),
  },
  {
    id: "view.channelRack",
    title: "Channel rack",
    section: "View",
    defaultShortcut: "Alt+1",
    keywords: "show step sequencer",
    checked: (state) => state.ui.centerTab === "channelRack",
    run: () => ui().showCenterTab("channelRack"),
  },
  {
    id: "view.playlist",
    title: "Playlist",
    section: "View",
    defaultShortcut: "Alt+2",
    keywords: "show song timeline arrangement",
    checked: (state) => state.ui.centerTab === "playlist",
    run: () => ui().showCenterTab("playlist"),
  },
  {
    id: "view.pianoRoll",
    title: "Piano roll",
    section: "View",
    defaultShortcut: "Alt+3",
    keywords: "show notes",
    checked: (state) => state.ui.centerTab === "pianoRoll",
    run: () => ui().showCenterTab("pianoRoll"),
  },
  {
    id: "view.toggleTheme",
    title: "Switch between light and dark",
    section: "View",
    defaultShortcut: "Mod+Shift+L",
    keywords: "theme appearance",
    run: () => ui().toggleTheme(),
  },
  {
    id: "view.resetLayout",
    title: "Reset layout",
    section: "View",
    keywords: "panels default restore",
    run: () => ui().resetLayout(),
  },

  {
    id: "options.settings",
    title: "Settings…",
    section: "Options",
    defaultShortcut: "Mod+Comma",
    keywords: "preferences audio device theme keymap",
    run: () => ui().openDialog("settings"),
  },
  {
    id: "engine.copyDeviceInfo",
    title: "Copy audio device info",
    section: "Options",
    keywords: "driver output sample rate buffer latency clipboard report",
    enabled: () => useEngineStore.getState().status !== null,
    run: copyDeviceInfo,
  },
  {
    id: "options.keymapWindfall",
    title: "Use Windfall shortcuts",
    section: "Options",
    keywords: "keymap keyboard preset",
    checked: (state) => state.ui.keymap === "windfall",
    run: () => ui().setKeymap("windfall"),
  },
  {
    id: "options.keymapFl",
    title: "Use FL Studio shortcuts",
    section: "Options",
    keywords: "keymap keyboard preset",
    checked: (state) => state.ui.keymap === "fl",
    run: () => ui().setKeymap("fl"),
  },

  {
    id: "help.gettingStarted",
    title: "Getting started",
    section: "Help",
    keywords: "tutorial first track basics",
    run: openGettingStarted,
  },
  {
    id: "help.shortcuts",
    title: "Keyboard shortcuts",
    section: "Help",
    keywords: "keys keymap",
    run: () => ui().openDialog("palette"),
  },
  {
    id: "help.about",
    title: "About Windfall",
    section: "Help",
    keywords: "version license",
    run: () => {
      toast("Windfall 0.1.0", {
        description: "A free, open source DAW. GPL-3.0.",
      })
    },
  },
]

export const RECENT_SECTION = "Recent projects"

function recentActions(paths: string[]): Action[] {
  return paths.map((path, index) => ({
    id: `file.openRecent.${index}`,
    title: fileName(path),
    section: RECENT_SECTION,
    keywords: `open ${path}`,
    run: () => openProjectPath(path),
  }))
}

let removeRecent: (() => void) | null = null

/** Keeps one "open recent" action per recent project file. */
export function syncRecentActions(paths: string[]) {
  removeRecent?.()
  removeRecent =
    paths.length > 0 ? registry.register(recentActions(paths)) : null
}

export function registerBuiltinActions(): () => void {
  const recordingSubscription = useRecordingStore.subscribe(() =>
    registry.invalidate()
  )
  const remove = registry.register(BUILTIN_ACTIONS, {
    presets: { fl: FL_KEYMAP },
  })
  return () => {
    remove()
    recordingSubscription()
    removeRecent?.()
    removeRecent = null
  }
}
