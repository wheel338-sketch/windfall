import { dynamicTitle, translatedText as t } from "@/lib/i18n"
import type { Channel } from "@/bindings"
import { INSTRUMENT_KINDS, instrumentDescriptor } from "@/features/params"
import {
  getAppState,
  invalidateActionsOn,
  registry,
  runAction,
  type Action,
  type AppState,
} from "@/lib/actions"
import { instrumentParams } from "@/lib/channel-source"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { selectedPatternId } from "@/lib/store/selectors"
import { useUiStore } from "@/lib/store/ui"

import {
  addChannelFromPickedFile,
  addInstrumentChannel,
  clearSteps,
  deleteChannel,
  duplicateChannel,
  fillEvery,
  initInstrument,
  loadInstrumentSound,
  moveChannelBy,
  renameChannel,
  replaceSampleFromPickedFile,
  resetChannelLevels,
  routeToNewTrack,
  selectChannel,
  selectedChannel,
  setPatternLength,
  shiftSteps,
  showInMixer,
  toggleMute,
  toggleSolo,
  unmuteAllChannels,
  unsoloAllChannels,
} from "./channel-ops"
import {
  focusNotePreviewTarget,
  selectedNotePreviewLane,
} from "./note-preview-target"
import { rackNoteView, useRackStore, type RackNoteView } from "./rack-store"
import { nextFlaggedChannelId } from "./select-next"
import { matchingPreset, SYNTH_PRESETS } from "./synth/presets"
import { openAdvancedFill } from "./advanced-fill"
import { dispatch } from "@/lib/store/project"
import { createChannelGroup, currentNamedGroup, openChannelGroups, removeChannelGroup, renameChannelGroup } from "./channel-groups"
import { addLayerChannel } from "./layer"

/** The palette section, and menu, of the built-in instrument sounds. */
export const SOUNDS_SECTION = "Sounds"

export const LENGTH_PRESETS = [16, 32, 48, 64]
export const FILL_INTERVALS = [2, 4, 8]

export const NOTE_VIEW_ACTION_IDS = [
  "channelRack.noteView.auto",
  "channelRack.noteView.steps",
  "channelRack.noteView.notes",
] as const
export const OPEN_NOTE_PREVIEW_ACTION = "channelRack.openPianoRoll"

const NOTE_VIEW_ACTIONS: Action[] = (
  [
    ["auto", "Automatic steps or notes", "Mod+Alt+1"],
    ["steps", "Show steps", "Mod+Alt+2"],
    ["notes", "Show notes", "Mod+Alt+3"],
  ] satisfies [RackNoteView, string, string][]
).map(([view, title, defaultShortcut], index) => ({
  id: NOTE_VIEW_ACTION_IDS[index],
  title,
  defaultShortcut,
  section: "Channels",
  scope: "channelRack",
  keywords: "row view note preview thumbnail piano roll step buttons",
  enabled: (state) => selectedNotePreviewLane(state) !== null,
  whyDisabled: () => t("Select a channel in a pattern"),
  checked: (state) => {
    const lane = selectedNotePreviewLane(state)
    return lane !== null && rackNoteView(useRackStore.getState(), lane) === view
  },
  run: () => {
    const lane = selectedNotePreviewLane(getAppState())
    if (!lane) return
    const target = { ...lane, generation: getProjectGeneration() }
    useRackStore.getState().setNoteView(lane, view)
    focusNotePreviewTarget(target)
  },
}))

function selected(state: AppState): Channel | undefined {
  return state.document.project.channels.find(
    (channel) => channel.id === state.ui.selectedChannel
  )
}

function selectedIndex(state: AppState): number {
  return state.document.project.channels.findIndex(
    (channel) => channel.id === state.ui.selectedChannel
  )
}

function selectedNoteCount(state: AppState): number {
  const channel = selected(state)
  if (!channel) return 0
  const { project } = state.document
  const patternId = selectedPatternId(project, state.transport.pattern)
  const pattern = project.patterns.find((item) => item.id === patternId)
  return (
    pattern?.lanes.find((lane) => lane.channel === channel.id)?.notes.length ??
    0
  )
}

function patternLength(state: AppState): number | undefined {
  const { project } = state.document
  const patternId = selectedPatternId(project, state.transport.pattern)
  return project.patterns.find((item) => item.id === patternId)?.lengthSteps
}

/** Runs `work` on the selected channel, if there is one. */
function withSelected(work: (channel: Channel) => void | Promise<void>) {
  return () => {
    const channel = selectedChannel()
    if (channel) return work(channel)
  }
}

const hasSelection = (state: AppState) => selected(state) !== undefined

const isSampler = (state: AppState) =>
  selected(state)?.source.type === "sampler"
const isInstrument = (state: AppState) =>
  selected(state)?.source.type === "instrument" &&
  !state.document.project.plugins?.some(
    (plugin) =>
      plugin.target.type === "instrument" &&
      plugin.target.channel === state.ui.selectedChannel
  )

/** The selected channel's synth settings, when it is a subtractive synth. */
function selectedSynth(state: AppState) {
  if (!isInstrument(state)) return null
  const params = instrumentParams(selected(state))
  return params?.type === "subtractiveSynth" ? params : null
}

/** Says which kind of channel an action is for, when the other is selected. */
const onlyFor =
  (wanted: "sampler" | "instrument", reason: () => string) =>
  (state: AppState): string | undefined => {
    const channel = selected(state)
    return channel && channel.source.type !== wanted ? reason() : undefined
  }

const ADD_INSTRUMENT_ACTIONS = INSTRUMENT_KINDS.map((kind): Action => ({
  id: `channel.addInstrument.${kind}`,
  ...dynamicTitle("Add {value0}", { value0: instrumentDescriptor(kind).name }, { value0: "lower" }),
  section: "Channels",
  keywords: "new channel instrument synthesizer synth keys bass lead pad",
  run: () => addInstrumentChannel(kind),
}))

/** The id of the action that loads a built-in synth sound. */
export const synthSoundActionId = (preset: string) =>
  `channel.synthSound.${preset}`

/** Every built-in synth sound but the init sound, which has its own action. */
const SYNTH_SOUND_ACTIONS = SYNTH_PRESETS.filter(
  (preset) => preset.id !== "init"
).map((preset): Action => ({
  id: synthSoundActionId(preset.id),
  ...dynamicTitle("Load synth sound: {value0}", { value0: preset.name }),
  section: SOUNDS_SECTION,
  keywords: `preset patch ${preset.description}`,
  enabled: (state) => selectedSynth(state) !== null,
  whyDisabled: () => t("Synth channels only"),
  checked: (state) => {
    const params = selectedSynth(state)
    return params !== null && matchingPreset(params)?.id === preset.id
  },
  run: withSelected((channel) =>
    loadInstrumentSound(channel.id, preset.params, `Load sound: ${preset.name}`)
  ),
}))

/**
 * What can be done to the selected channel and to the pattern's steps. A
 * row's right-click menu selects the row and then lists these. The keys
 * belong to the rack: they work while it has the keyboard.
 */
export const CHANNEL_RACK_ACTIONS: Action[] = [
  ...NOTE_VIEW_ACTIONS,
  {
    id: OPEN_NOTE_PREVIEW_ACTION,
    ...dynamicTitle("Open in piano roll", {}),
    section: "Channels",
    scope: "channelRack",
    keywords: "row note preview thumbnail edit notes",
    standsFor: () => "view.pianoRoll",
    enabled: (state) => selectedNotePreviewLane(state) !== null,
    whyDisabled: () => t("Select a channel in a pattern"),
    run: () => runAction("view.pianoRoll"),
  },
  {
    id: "channelRack.graph",
    ...dynamicTitle("Step graph editor", {}),
    section: "View",
    scope: "channelRack",
    defaultShortcut: "Alt+G",
    keywords: "velocity pan note pitch length timing shift bars paint properties",
    enabled: (state) => selectedNotePreviewLane(state) !== null,
    whyDisabled: () => t("Select a channel in a pattern"),
    checked: () => useRackStore.getState().graphOpen,
    run: () => useRackStore.getState().setGraphOpen(!useRackStore.getState().graphOpen),
  },
  {
    id: "channel.resetTiming",
    ...dynamicTitle("Reset channel note timing", {}),
    section: "Channels",
    keywords: "gate shift swing mix reset defaults",
    enabled: hasSelection,
    run: withSelected((channel) => dispatch({ type: "updateChannel", id: channel.id, patch: { timing: { swingMix: 1, gateTicks: 0, shiftTicks: 0 } } }).then(() => {})),
  },
  {
    id: "channelRack.groups",
    ...dynamicTitle("Channel groups…", {}),
    section: "Channels",
    keywords: "organize filter category assign multiple channels",
    run: openChannelGroups,
  },
  {
    id: "channelRack.createGroup",
    ...dynamicTitle("Create channel group…", {}),
    section: "Channels",
    keywords: "organize filter category new group",
    enabled: hasSelection,
    run: createChannelGroup,
  },
  {
    id: "channelRack.renameGroup",
    ...dynamicTitle("Rename channel group…", {}),
    section: "Channels",
    keywords: "organize filter category merge groups",
    enabled: () => currentNamedGroup() !== null,
    whyDisabled: () => t("Select a named group or a grouped channel"),
    run: renameChannelGroup,
  },
  {
    id: "channelRack.removeGroup",
    ...dynamicTitle("Remove channel group…", {}),
    section: "Channels",
    keywords: "organize filter category ungroup channels",
    enabled: () => currentNamedGroup() !== null,
    whyDisabled: () => t("Select a named group or a grouped channel"),
    run: removeChannelGroup,
  },
  {
    id: "channelRack.showAllGroups",
    ...dynamicTitle("Show all channel groups", {}),
    section: "Channels",
    keywords: "organize filter reveal all channels",
    checked: () => useRackStore.getState().groupFilter === null,
    run: () => useRackStore.getState().setGroupFilter(null),
  },
  ...NOTE_VIEW_ACTIONS,
  {
    id: OPEN_NOTE_PREVIEW_ACTION,
    ...dynamicTitle("Open in piano roll", {}),
    section: "Channels",
    scope: "channelRack",
    keywords: "row note preview thumbnail edit notes",
    standsFor: () => "view.pianoRoll",
    enabled: (state) => selectedNotePreviewLane(state) !== null,
    whyDisabled: () => t("Select a channel in a pattern"),
    run: () => runAction("view.pianoRoll"),
  },
  {
    id: "channel.sendStepsToPianoRoll",
    ...dynamicTitle("Send steps to piano roll", {}),
    section: "Channels",
    scope: "channelRack",
    keywords: "convert step sequencer edit notes piano",
    standsFor: () => "view.pianoRoll",
    enabled: (state) => selectedNotePreviewLane(state) !== null,
    whyDisabled: () => t("Select a channel in a pattern"),
    run: async () => {
      const lane = selectedNotePreviewLane(getAppState())
      if (!lane) return
      // Step notes already are piano notes. Select their existing lane;
      // creating copies here would double playback and discard properties.
      useRackStore.getState().setNoteView(lane, "notes")
      await runAction("view.pianoRoll")
    },
  },
  {
    id: "channel.addFromFile",
    ...dynamicTitle("Add channel from an audio file…", {}),
    section: "Channels",
    keywords: "new sampler sample load open wav",
    run: addChannelFromPickedFile,
  },
  ...ADD_INSTRUMENT_ACTIONS,
  {
    id: "channel.addLayer",
    ...dynamicTitle("Add layer channel", {}),
    section: "Channels",
    keywords: "new layer stack children notes together silent fan out",
    run: () => addLayerChannel(),
  },
  {
    id: "channel.replaceSample",
    ...dynamicTitle("Replace sample from an audio file…", {}),
    section: "Channels",
    keywords: "swap sound load sampler",
    enabled: isSampler,
    whyDisabled: onlyFor("sampler", () => t("Samplers only")),
    run: withSelected((channel) => replaceSampleFromPickedFile(channel.id)),
  },
  {
    id: "channel.initInstrument",
    ...dynamicTitle("Init instrument", {}),
    section: "Channels",
    keywords: "reset default sound preset patch clear synth",
    enabled: isInstrument,
    whyDisabled: onlyFor("instrument", () => t(t("Instruments only"))),
    run: withSelected((channel) => initInstrument(channel.id)),
  },
  ...SYNTH_SOUND_ACTIONS,
  {
    id: "channel.rename",
    ...dynamicTitle("Rename channel…", {}),
    section: "Channels",
    scope: "channelRack",
    defaultShortcut: "F2",
    enabled: hasSelection,
    run: withSelected((channel) => renameChannel(channel.id)),
  },
  {
    id: "channel.color",
    ...dynamicTitle("Change channel color…", {}),
    section: "Channels",
    keywords: "colour swatch",
    enabled: hasSelection,
    run: withSelected((channel) => {
      useUiStore.getState().showCenterTab("channelRack")
      useRackStore.getState().openColorPicker(channel.id)
    }),
  },
  {
    id: "channel.duplicate",
    ...dynamicTitle("Duplicate channel", {}),
    section: "Channels",
    scope: "channelRack",
    editCommand: "duplicate",
    defaultShortcut: "Mod+D",
    keywords: "clone copy",
    enabled: hasSelection,
    run: withSelected((channel) => duplicateChannel(channel.id)),
  },
  {
    id: "channel.delete",
    ...dynamicTitle("Delete channel…", {}),
    section: "Channels",
    scope: "channelRack",
    editCommand: "delete",
    defaultShortcut: "Delete",
    keywords: "remove",
    enabled: hasSelection,
    run: withSelected((channel) => deleteChannel(channel.id)),
  },
  {
    id: "channel.moveUp",
    ...dynamicTitle("Move channel up", {}),
    section: "Channels",
    scope: "channelRack",
    defaultShortcut: "Alt+ArrowUp",
    repeats: true,
    keywords: "reorder",
    enabled: (state) => selectedIndex(state) > 0,
    run: withSelected((channel) => moveChannelBy(channel.id, -1)),
  },
  {
    id: "channel.moveDown",
    ...dynamicTitle("Move channel down", {}),
    section: "Channels",
    scope: "channelRack",
    defaultShortcut: "Alt+ArrowDown",
    repeats: true,
    keywords: "reorder",
    enabled: (state) => {
      const index = selectedIndex(state)
      return index >= 0 && index < state.document.project.channels.length - 1
    },
    run: withSelected((channel) => moveChannelBy(channel.id, 1)),
  },
  {
    id: "channel.mute",
    ...dynamicTitle("Mute channel", {}),
    section: "Channels",
    keywords: "silence lamp",
    enabled: hasSelection,
    checked: (state) => selected(state)?.muted ?? false,
    run: withSelected((channel) => toggleMute(channel.id)),
  },
  {
    id: "channel.solo",
    ...dynamicTitle("Solo channel", {}),
    section: "Channels",
    keywords: "only hear",
    enabled: hasSelection,
    checked: (state) => selected(state)?.solo ?? false,
    run: withSelected((channel) => toggleSolo(channel.id)),
  },
  {
    id: "channel.unmuteAll",
    ...dynamicTitle("Unmute all", {}),
    section: "Channels",
    enabled: (state) =>
      state.document.project.channels.some((channel) => channel.muted),
    run: unmuteAllChannels,
  },
  {
    id: "channel.unsoloAll",
    ...dynamicTitle("Unsolo all", {}),
    section: "Channels",
    enabled: (state) =>
      state.document.project.channels.some((channel) => channel.solo),
    run: unsoloAllChannels,
  },
  {
    id: "channel.resetLevels",
    ...dynamicTitle("Reset levels", {}),
    section: "Channels",
    enabled: (state) =>
      state.document.project.channels.some(
        (channel) => channel.volume !== 1 || channel.pan !== 0
      ),
    run: resetChannelLevels,
  },
  {
    id: "channel.selectNextMuted",
    ...dynamicTitle("Select next muted channel", {}),
    section: "Channels",
    enabled: (state) =>
      nextFlaggedChannelId(
        state.document.project.channels,
        state.ui.selectedChannel,
        "muted"
      ) !== null,
    run: () => {
      const state = getAppState()
      const id = nextFlaggedChannelId(
        state.document.project.channels,
        state.ui.selectedChannel,
        "muted"
      )
      if (id !== null) selectChannel(id)
    },
  },
  {
    id: "channel.selectNextSolo",
    ...dynamicTitle("Select next solo channel", {}),
    section: "Channels",
    enabled: (state) =>
      nextFlaggedChannelId(
        state.document.project.channels,
        state.ui.selectedChannel,
        "solo"
      ) !== null,
    run: () => {
      const state = getAppState()
      const id = nextFlaggedChannelId(
        state.document.project.channels,
        state.ui.selectedChannel,
        "solo"
      )
      if (id !== null) selectChannel(id)
    },
  },
  {
    id: "channel.clearSteps",
    ...dynamicTitle("Clear steps", {}),
    section: "Channels",
    keywords: "erase notes empty row",
    enabled: (state) => selectedNoteCount(state) > 0,
    run: withSelected((channel) => clearSteps(channel.id)),
  },
  ...FILL_INTERVALS.map((every): Action => ({
    id: `channel.fill${every}`,
    ...dynamicTitle("Fill every {value0} steps", { value0: every }),
    section: "Channels",
    keywords: "pattern hits repeat",
    enabled: hasSelection,
    run: withSelected((channel) => fillEvery(channel.id, every)),
  })),
  {
    id: "channel.advancedFill",
    ...dynamicTitle("Advanced step fill…", {}),
    section: "Channels",
    keywords: "rhythm euclidean random seeded pulses preview drum pattern",
    enabled: (state) =>
      hasSelection(state) && patternLength(state) !== undefined,
    whyDisabled: () => t("Select a channel in a pattern"),
    run: openAdvancedFill,
  },
  {
    id: "channel.shiftLeft",
    ...dynamicTitle("Shift steps left", {}),
    section: "Channels",
    scope: "channelRack",
    defaultShortcut: "Mod+Shift+ArrowLeft",
    repeats: true,
    keywords: "rotate nudge earlier",
    enabled: (state) => selectedNoteCount(state) > 0,
    run: withSelected((channel) => shiftSteps(channel.id, -1)),
  },
  {
    id: "channel.shiftRight",
    ...dynamicTitle("Shift steps right", {}),
    section: "Channels",
    scope: "channelRack",
    defaultShortcut: "Mod+Shift+ArrowRight",
    repeats: true,
    keywords: "rotate nudge later",
    enabled: (state) => selectedNoteCount(state) > 0,
    run: withSelected((channel) => shiftSteps(channel.id, 1)),
  },
  {
    id: "channel.routeToNewTrack",
    ...dynamicTitle("Route to a new mixer track", {}),
    section: "Channels",
    keywords: "insert output send",
    enabled: hasSelection,
    run: withSelected((channel) => routeToNewTrack(channel.id)),
  },
  {
    id: "channel.showInMixer",
    ...dynamicTitle("Show the channel's mixer track", {}),
    section: "Channels",
    keywords: "routing jump go to",
    enabled: hasSelection,
    run: withSelected((channel) => showInMixer(channel.id)),
  },
  {
    id: "channelRack.settings",
    ...dynamicTitle("Channel settings", {}),
    section: "View",
    keywords: "show hide toggle inspector sampler panel",
    checked: () => useRackStore.getState().inspectorOpen,
    run: () => {
      useUiStore.getState().showCenterTab("channelRack")
      useRackStore.getState().toggleInspector()
    },
  },
  ...LENGTH_PRESETS.map((steps): Action => ({
    id: `pattern.length${steps}`,
    ...dynamicTitle("Set the pattern length to {value0} steps", { value0: steps }),
    section: "Patterns",
    keywords: "bars steps long",
    checked: (state) => patternLength(state) === steps,
    run: () => setPatternLength(steps),
  })),
]

/** Adds the rack's actions to the registry. Returns a function that removes them. */
export function registerChannelRackActions(): () => void {
  const stops = [
    registry.register(CHANNEL_RACK_ACTIONS),
    invalidateActionsOn(useRackStore, (state) => [
      state.inspectorOpen,
      state.noteViews,
      state.groupFilter,
      state.graphOpen,
    ]),
    onProjectReplaced(() => registry.invalidate()),
  ]
  return () => {
    for (const stop of stops) stop()
  }
}
