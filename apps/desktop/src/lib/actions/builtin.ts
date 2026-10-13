import { dynamicTitle, message, plainNumber, translatedText as t } from "@/lib/i18n"
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
    return message("No audio: {error}", { error: status.error ?? t("the output is not running") })
  }
  const plugins =
    status.latencyFrames > 0 ? message(", plus {latencyFrames} samples", { latencyFrames: plainNumber(status.latencyFrames) }) : ""
  return message("{host} {device}, {rate}, {buffer} samples, {latency} ms{plugins}", { host: status.host, device: status.device ?? t("default device"), rate: formatSampleRate(status.sampleRate), buffer: plainNumber(status.bufferFrames), latency: status.latencyMs.toFixed(1), plugins })
}

async function copyDeviceInfo() {
  const status = useEngineStore.getState().status
  if (!status) return
  const text = deviceInfo(status)
  await navigator.clipboard.writeText(text)
  toast.success(t("Copied"), { description: text })
}

const EDIT_COMMANDS: { command: EditCommand; title: string; words: string }[] =
  [
    { command: "cut", get title() { return t("Cut") }, words: "clipboard move" },
    { command: "copy", get title() { return t("Copy") }, words: "clipboard" },
    { command: "paste", get title() { return t("Paste") }, words: "clipboard insert" },
    { command: "duplicate", get title() { return t("Duplicate") }, words: "clone repeat" },
    { command: "delete", get title() { return t("Delete") }, words: "remove erase" },
    { command: "selectAll", get title() { return t("Select all") }, words: "everything" },
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
    ...dynamicTitle("Import MIDI…", {}),
    section: "File",
    keywords: "mid append notes arrangement",
    run: () => ui().openDialog("midiImport"),
  },
  {
    id: "file.exportMidi",
    ...dynamicTitle("Export MIDI…", {}),
    section: "File",
    keywords: "mid notes tempo pattern song",
    run: () => ui().openDialog("midiExport"),
  },
  {
    id: "recording.open",
    ...dynamicTitle("Record audio input", {}),
    section: "Transport",
    run: openRecording,
  },
  {
    id: "audioLogger.open",
    ...dynamicTitle("Audio logger…", {}),
    section: "Transport",
    keywords: "background history recover take capture retrospective log",
    run: openAudioLogger,
  },
  {
    id: "recording.start",
    ...dynamicTitle("Start recording", {}),
    section: "Transport",
    enabled: () =>
      !useRecordingStore.getState().busy &&
      !useRecordingStore.getState().state.active &&
      useRecordingStore.getState().inputs.length > 0,
    run: startRecording,
  },
  {
    id: "recording.stop",
    ...dynamicTitle("Stop recording and keep take", {}),
    section: "Transport",
    enabled: () =>
      !useRecordingStore.getState().busy &&
      useRecordingStore.getState().state.active,
    run: stopRecording,
  },
  {
    id: "recording.cancel",
    ...dynamicTitle("Discard recording", {}),
    section: "Transport",
    enabled: () =>
      !useRecordingStore.getState().busy &&
      useRecordingStore.getState().state.active,
    run: cancelRecording,
  },

  {
    id: "file.new",
    ...dynamicTitle("New project", {}),
    section: "File",
    defaultShortcut: "Mod+N",
    run: newProject,
  },
  {
    id: "file.open",
    ...dynamicTitle("Open project…", {}),
    section: "File",
    defaultShortcut: "Mod+O",
    run: openProject,
  },
  {
    id: "file.importFlp",
    ...dynamicTitle("Import FL Studio project…", {}),
    section: "File",
    keywords: "flp convert notes arrangement samples",
    run: () => ui().openDialog("flpImport"),
  },
  {
    id: "file.save",
    ...dynamicTitle("Save", {}),
    section: "File",
    defaultShortcut: "Mod+S",
    keywords: "write project",
    run: async () => {
      await saveProject()
    },
  },
  {
    id: "file.saveAs",
    ...dynamicTitle("Save as…", {}),
    section: "File",
    defaultShortcut: "Mod+Shift+S",
    run: async () => {
      await saveProjectAs()
    },
  },
  {
    id: "file.saveNewVersion",
    ...dynamicTitle("Save new version", {}),
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
    ...dynamicTitle("Export portable project archive…", {}),
    section: "File",
    keywords: "zip package samples backup share",
    enabled: canArchive,
    whyDisabled: portableUnavailable,
    run: saveArchive,
  },
  {
    id: "file.cancelArchive",
    ...dynamicTitle("Cancel project archive", {}),
    section: "File",
    enabled: () => useArchiveStore.getState().cancellable,
    run: cancelArchive,
  },
  {
    id: "file.reloadSamples",
    ...dynamicTitle("Reload missing samples", {}),
    section: "File",
    keywords: "retry find relink audio files warnings",
    run: reloadMissingSamples,
  },
  {
    id: "file.export",
    ...dynamicTitle("Export audio…", {}),
    section: "File",
    defaultShortcut: "Mod+E",
    keywords: "render bounce wav",
    run: () => ui().openDialog("export"),
  },

  {
    id: "edit.undo",
    ...dynamicTitle("Undo", {}),
    section: "Edit",
    defaultShortcut: "Mod+Z",
    repeats: true,
    enabled: (state) => state.document.history.cursor > 0,
    run: undo,
  },
  {
    id: "edit.redo",
    ...dynamicTitle("Redo", {}),
    section: "Edit",
    defaultShortcut: ["Mod+Shift+Z", "Mod+Y"],
    repeats: true,
    enabled: (state) =>
      state.document.history.cursor < state.document.history.entries.length,
    run: redo,
  },
  {
    id: "edit.history",
    ...dynamicTitle("History", {}),
    section: "Edit",
    keywords: "steps list earlier back",
    run: () => ui().setHistoryOpen(true),
  },
  ...EDIT_COMMANDS.map(editAction),

  {
    id: "transport.toggle",
    ...dynamicTitle("Play or stop", {}),
    section: "Transport",
    defaultShortcut: "Space",
    keywords: "start pause playback",
    run: togglePlayback,
  },
  {
    id: "transport.play",
    ...dynamicTitle("Play", {}),
    section: "Transport",
    enabled: (state) => !state.transport.playing,
    run: play,
  },
  {
    id: "transport.stop",
    ...dynamicTitle("Stop", {}),
    section: "Transport",
    run: stop,
  },
  {
    id: "transport.metronome",
    ...dynamicTitle("Metronome", {}),
    section: "Transport",
    keywords: "click beat bar accent time signature",
    checked: (state) => state.transport.metronome?.enabled ?? false,
    enabled: () => !useRecordingStore.getState().state.active,
    run: () => setMetronome({ enabled: !useTransportStore.getState().metronome?.enabled }),
  },
  {
    id: "transport.patternMode",
    ...dynamicTitle("Play the pattern", {}),
    section: "Transport",
    keywords: "pattern mode loop",
    checked: (state) => state.transport.mode === "pattern",
    run: () => setPlayMode("pattern"),
  },
  {
    id: "transport.songMode",
    ...dynamicTitle("Play the song", {}),
    section: "Transport",
    keywords: "song mode playlist",
    checked: (state) => state.transport.mode === "song",
    run: () => setPlayMode("song"),
  },
  {
    id: "transport.toggleMode",
    ...dynamicTitle("Switch between pattern and song", {}),
    section: "Transport",
    defaultShortcut: "L",
    run: () =>
      setPlayMode(
        useTransportStore.getState().mode === "pattern" ? "song" : "pattern"
      ),
  },

  {
    id: "tempo.tap",
    ...dynamicTitle("Tap tempo", {}),
    section: "Transport",
    keywords: "bpm beat measure speed",
    run: tapTempo,
  },
  {
    id: "tempo.half",
    ...dynamicTitle("Halve the tempo", {}),
    section: "Transport",
    keywords: "bpm half time slower",
    enabled: (state) =>
      canScaleTempo(state.document.project.settings.tempoBpm, 0.5),
    run: () => scaleTempo(0.5),
  },
  {
    id: "tempo.double",
    ...dynamicTitle("Double the tempo", {}),
    section: "Transport",
    keywords: "bpm double time faster",
    enabled: (state) =>
      canScaleTempo(state.document.project.settings.tempoBpm, 2),
    run: () => scaleTempo(2),
  },
  {
    id: "tempo.reset",
    ...dynamicTitle("Reset the tempo to {value0} BPM", { value0: DEFAULT_TEMPO_BPM }),
    section: "Transport",
    keywords: "bpm default",
    enabled: (state) =>
      state.document.project.settings.tempoBpm !== DEFAULT_TEMPO_BPM,
    run: resetTempo,
  },

  {
    id: "channel.add",
    ...dynamicTitle("Add channel", {}),
    section: "Channels",
    defaultShortcut: "Alt+C",
    keywords: "new sampler instrument",
    run: addChannel,
  },

  {
    id: "pattern.add",
    ...dynamicTitle("Add pattern", {}),
    section: "Patterns",
    defaultShortcut: "Alt+P",
    keywords: "new",
    run: addPattern,
  },
  {
    id: "pattern.next",
    ...dynamicTitle("Next pattern", {}),
    section: "Patterns",
    defaultShortcut: "Mod+ArrowDown",
    enabled: (state) =>
      patternIndex(state) < state.document.project.patterns.length - 1,
    run: () => stepPattern(1),
  },
  {
    id: "pattern.previous",
    ...dynamicTitle("Previous pattern", {}),
    section: "Patterns",
    defaultShortcut: "Mod+ArrowUp",
    enabled: (state) => patternIndex(state) > 0,
    run: () => stepPattern(-1),
  },
  {
    id: "pattern.rename",
    ...dynamicTitle("Rename pattern…", {}),
    section: "Patterns",
    run: () => renamePattern(),
  },
  {
    id: "pattern.duplicate",
    ...dynamicTitle("Duplicate pattern", {}),
    section: "Patterns",
    keywords: "clone copy",
    run: () => duplicatePattern(),
  },
  {
    id: "pattern.delete",
    ...dynamicTitle("Delete pattern", {}),
    section: "Patterns",
    keywords: "remove",
    enabled: (state) => state.document.project.patterns.length > 1,
    run: () => deletePattern(),
  },

  {
    id: "view.commandPalette",
    ...dynamicTitle("Command palette…", {}),
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
    ...dynamicTitle("Browser", {}),
    section: "View",
    defaultShortcut: "Mod+B",
    keywords: "show hide toggle samples files panel",
    checked: (state) => state.ui.panels.browser,
    run: () => ui().togglePanel("browser"),
  },
  {
    id: "view.mixer",
    ...dynamicTitle("Mixer", {}),
    section: "View",
    defaultShortcut: "Mod+J",
    keywords: "show hide toggle panel",
    checked: (state) => state.ui.panels.mixer,
    run: () => ui().togglePanel("mixer"),
  },
  {
    id: "view.channelRack",
    ...dynamicTitle("Channel rack", {}),
    section: "View",
    defaultShortcut: "Alt+1",
    keywords: "show step sequencer",
    checked: (state) => state.ui.centerTab === "channelRack",
    run: () => ui().showCenterTab("channelRack"),
  },
  {
    id: "view.playlist",
    ...dynamicTitle("Playlist", {}),
    section: "View",
    defaultShortcut: "Alt+2",
    keywords: "show song timeline arrangement",
    checked: (state) => state.ui.centerTab === "playlist",
    run: () => ui().showCenterTab("playlist"),
  },
  {
    id: "view.pianoRoll",
    ...dynamicTitle("Piano roll", {}),
    section: "View",
    defaultShortcut: "Alt+3",
    keywords: "show notes",
    checked: (state) => state.ui.centerTab === "pianoRoll",
    run: () => ui().showCenterTab("pianoRoll"),
  },
  {
    id: "view.toggleTheme",
    ...dynamicTitle("Switch between light and dark", {}),
    section: "View",
    defaultShortcut: "Mod+Shift+L",
    keywords: "theme appearance",
    run: () => ui().toggleTheme(),
  },
  {
    id: "view.resetLayout",
    ...dynamicTitle("Reset layout", {}),
    section: "View",
    keywords: "panels default restore",
    run: () => ui().resetLayout(),
  },

  {
    id: "options.settings",
    ...dynamicTitle("Settings…", {}),
    section: "Options",
    defaultShortcut: "Mod+Comma",
    keywords: "preferences audio device theme keymap",
    run: () => ui().openDialog("settings"),
  },
  {
    id: "engine.copyDeviceInfo",
    ...dynamicTitle("Copy audio device info", {}),
    section: "Options",
    keywords: "driver output sample rate buffer latency clipboard report",
    enabled: () => useEngineStore.getState().status !== null,
    run: copyDeviceInfo,
  },
  {
    id: "options.keymapWindfall",
    ...dynamicTitle("Use Windfall shortcuts", {}),
    section: "Options",
    keywords: "keymap keyboard preset",
    checked: (state) => state.ui.keymap === "windfall",
    run: () => ui().setKeymap("windfall"),
  },
  {
    id: "options.keymapFl",
    ...dynamicTitle("Use FL Studio shortcuts", {}),
    section: "Options",
    keywords: "keymap keyboard preset",
    checked: (state) => state.ui.keymap === "fl",
    run: () => ui().setKeymap("fl"),
  },

  {
    id: "help.gettingStarted",
    ...dynamicTitle("Getting started", {}),
    section: "Help",
    keywords: "tutorial first track basics",
    run: openGettingStarted,
  },
  {
    id: "help.shortcuts",
    ...dynamicTitle("Keyboard shortcuts", {}),
    section: "Help",
    keywords: "keys keymap",
    run: () => ui().openDialog("palette"),
  },
  {
    id: "help.about",
    ...dynamicTitle("About Windfall", {}),
    section: "Help",
    keywords: "version license",
    run: () => {
      toast("Windfall 0.1.0", {
        description: t("A free, open source DAW. GPL-3.0."),
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
