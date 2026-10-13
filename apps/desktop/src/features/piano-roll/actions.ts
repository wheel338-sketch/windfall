import { dynamicTitle } from "@/lib/i18n"
import {
  invalidateActionsOn,
  registry,
  type Action,
  type AppState,
  type PresetShortcuts,
} from "@/lib/actions"
import { dispatch } from "@/lib/store/project"

import { LANE_KINDS } from "./lane-math"
import { NOTE_TOOLS, openNoteTools } from "./note-tools"
import { currentSession } from "./session"
import { openNoteProperties, setSelectedArticulation, setSelectedColorGroup } from "./note-properties"
import { openNoteCurves } from "./note-curves"
import { NOTE_COLOR_GROUPS, noteColorGroupLabel } from "@/lib/note-colors"
import { NOTE_ARTICULATIONS } from "@/lib/note-expression"
import { openNoteLfo } from "@/features/automation/lfo-dialog"
import { SNAP_OPTIONS } from "./snap"
import { usePianoRollStore, type Tool } from "./store"
import { openWaveformHelper } from "./waveform-helper"
import { openChordDisplay } from "./chords/readout-panel"
import { openPatternTimeline } from "./pattern-timeline"
import { openRiffGenerator } from "./riff-generator"
import { openProgressionGenerator } from "./progression"
import { exportSheetMusic } from "./sheet-export"
import { useTypingKeyboardStore } from "./typing-keyboard"
import { useStepEntryStore } from "./step-entry"
import { canDumpPlayedNotes, dumpPlayedNotes } from "./dump-played-notes"
import { useNoteLogStore } from "./note-log"
import { rememberedVelocity } from "./pointer-tools"
import { restoredVelocities } from "./restore-muted"
import { notesAtTick } from "./select-playhead"
import { GLIDE_PRESETS, setSelectedGlide } from "./glide-presets"
import { setSelectedGlidePresetStep } from "./glide-preset-step"
import { setSelectedGlideScale } from "./glide-scale"
import { LENGTH_PRESETS, setSelectedLength } from "./length-presets"
import { setSelectedNoteLengthPresetStep } from "./length-preset-step"
import { setSelectedLengthScale } from "./length-scale"
import { setSelectedNoteStartScale } from "./note-start-scale"
import { setSelectedNoteKeyScale } from "./note-key-scale"
import { setSelectedNoteFromEndScale } from "./note-from-end-scale"

const SECTION = "Piano roll"

const roll = () => usePianoRollStore.getState()
const editor = () => currentSession()?.editor

/*
 * These act on the piano roll in view. What is selected is not part of the
 * state actions are handed; it is read from the piano roll's own store,
 * which `registerPianoRollActions` makes the registry follow.
 */
const inRoll = (state: AppState) => state.ui.centerTab === "pianoRoll"
const hasSelection = (state: AppState) =>
  inRoll(state) && roll().selectionCount > 0
const hasNotes = (state: AppState) =>
  inRoll(state) && (editor()?.notes.length ?? 0) > 0

function restorableNotes() {
  const context = editor()?.context
  if (!context) return []
  return restoredVelocities(context.notes, (id) =>
    rememberedVelocity(context.pattern.id, context.channel, id)
  )
}

const TOOL_ACTIONS: {
  tool: Tool
  title: string
  key: string
  words: string
}[] = [
  { tool: "draw", title: "Draw tool", key: "D", words: "pencil add notes" },
  { tool: "paint", title: "Paint tool", key: "B", words: "brush repeat" },
  { tool: "select", title: "Select tool", key: "S", words: "marquee box" },
  { tool: "erase", title: "Erase tool", key: "E", words: "delete rubber" },
  { tool: "mute", title: "Mute tool", key: "M", words: "silence unmute note" },
  { tool: "slice", title: "Slice tool", key: "C", words: "split cut note" },
  { tool: "zoom", title: "Zoom tool", key: "Z", words: "rectangle viewport" },
  { tool: "playback", title: "Playback tool", key: "Y", words: "scrub seek audition" },
]

function capital(word: string): string {
  return word.charAt(0).toUpperCase() + word.slice(1)
}

const ACTIONS: Action[] = [
  {
    id: "pianoRoll.dumpPlayedNotes",
    title: "Dump played notes",
    section: SECTION,
    keywords: "audition log recent held notes capture",
    enabled: (state) => inRoll(state) && canDumpPlayedNotes(),
    whyDisabled: () => "Open a pattern channel and audition notes on it",
    run: dumpPlayedNotes,
  },
  {
    id: "pianoRoll.stepEntry",
    title: "Step entry",
    section: SECTION,
    defaultShortcut: "\\",
    keywords: "computer keyboard insert note snap cursor",
    enabled: inRoll,
    checked: () => useStepEntryStore.getState().enabled,
    run: () => {
      const stepEntry = useStepEntryStore.getState()
      stepEntry.setEnabled(!stepEntry.enabled)
      currentSession()?.focusGrid()
    },
  },
  {
    id: "pianoRoll.typing",
    title: "Typing",
    section: SECTION,
    defaultShortcut: "`",
    keywords: "computer keyboard audition octave",
    enabled: inRoll,
    checked: () => useTypingKeyboardStore.getState().enabled,
    run: () => {
      const typing = useTypingKeyboardStore.getState()
      typing.setEnabled(!typing.enabled)
      currentSession()?.focusGrid()
    },
  },
  {
    id: "pianoRoll.exportSheetMusic",
    title: "Export sheet music…",
    section: SECTION,
    keywords: "MusicXML PDF print notation score staff engrave save channel",
    enabled: (state) => inRoll(state) && !!editor()?.context,
    whyDisabled: () => "Open a pattern channel in the piano roll",
    run: exportSheetMusic,
  },
  {
    id: "pianoRoll.generateRiff",
    title: "Generate riff…",
    section: SECTION,
    keywords: "seed melody scale append generator",
    enabled: (state) => inRoll(state) && !!editor()?.context && !editor()?.busy,
    run: openRiffGenerator,
  },
  {
    id: "pianoRoll.generateProgression",
    title: "Generate progression…",
    section: SECTION,
    keywords: "seed chord progression triad diatonic mood append generator",
    enabled: (state) => inRoll(state) && !!editor()?.context && !editor()?.busy,
    run: openProgressionGenerator,
  },
  {
    id: "pianoRoll.patternTimeline",
    title: "Pattern markers and time signatures…",
    section: SECTION,
    keywords: "local meter change timeline pattern ruler",
    enabled: inRoll,
    run: () => openPatternTimeline(),
  },
  {
    id: "pianoRoll.waveformHelper",
    title: "Waveform helper…",
    section: SECTION,
    keywords: "audio waveform background timing reference",
    enabled: inRoll,
    run: openWaveformHelper,
  },
  {
    id: "pianoRoll.chordDisplay",
    title: "Chord display",
    section: SECTION,
    keywords: "chord name detect played selected playhead symbol inversion",
    enabled: inRoll,
    run: openChordDisplay,
  },
  {
    id: "pianoRoll.lfo",
    title: "Write note-event LFO…",
    section: SECTION,
    keywords: "sine triangle pulse random sample hold modulation event lane",
    enabled: hasNotes,
    run: openNoteLfo,
  },
  ...[null, ...NOTE_COLOR_GROUPS.map((_, group) => group)].flatMap((group): Action[] => [
    {
      id: `pianoRoll.color.${group ?? "auto"}`,
      ...dynamicTitle("Set selected notes: {value0}", { value0: noteColorGroupLabel(group) }),
      section: SECTION,
      keywords: "note color group MIDI channel routing",
      enabled: hasSelection,
      run: () => setSelectedColorGroup(group),
    },
    {
      id: `pianoRoll.selectColor.${group ?? "auto"}`,
      ...dynamicTitle("Select notes: {value0}", { value0: noteColorGroupLabel(group) }),
      section: SECTION,
      keywords: "note color group MIDI channel selection",
      enabled: hasNotes,
      run: () => {
        const current = editor()
        if (!current || current.busy) return
        current.setSelection(current.notes.filter((note) => (note.expression?.colorGroup ?? null) === group).map((note) => note.id))
        currentSession()?.focusGrid()
      },
    },
  ]),
  ...NOTE_ARTICULATIONS.map((item): Action => ({
    id: `pianoRoll.articulation.${item.value}`,
    ...dynamicTitle("Set selected notes: {value0}", { value0: item.label.toLowerCase() }),
    section: SECTION,
    keywords: "slide portamento glide articulation pitch bend",
    enabled: hasSelection,
    run: () => setSelectedArticulation(item.value),
  })),
  ...GLIDE_PRESETS.map((item): Action => ({
    id: `pianoRoll.glide.${item.glideTicks}`,
    ...dynamicTitle("Set selected notes: {value0} portamento", { value0: item.label.toLowerCase() }),
    section: SECTION,
    keywords: "portamento duration glide ticks",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedGlide(item.glideTicks),
  })),
  {
    id: "pianoRoll.glide.previous",
    title: "Previous portamento preset",
    section: SECTION,
    keywords: "portamento duration glide preset",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedGlidePresetStep("previous"),
  },
  {
    id: "pianoRoll.glide.next",
    title: "Next portamento preset",
    section: SECTION,
    keywords: "portamento duration glide preset",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedGlidePresetStep("next"),
  },
  {
    id: "pianoRoll.glide.half",
    title: "Halve selected portamento",
    section: SECTION,
    keywords: "portamento duration glide half",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedGlideScale("half"),
  },
  {
    id: "pianoRoll.glide.double",
    title: "Double selected portamento",
    section: SECTION,
    keywords: "portamento duration glide double",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedGlideScale("double"),
  },
  ...LENGTH_PRESETS.map((item): Action => ({
    id: `pianoRoll.length.${item.length}`,
    ...dynamicTitle("Set selected notes: {value0}", { value0: item.label.toLowerCase() }),
    section: SECTION,
    keywords: "note length duration ticks",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedLength(item.length),
  })),
  {
    id: "pianoRoll.length.previous",
    title: "Previous length preset",
    section: SECTION,
    keywords: "note length duration preset",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedNoteLengthPresetStep("previous"),
  },
  {
    id: "pianoRoll.length.next",
    title: "Next length preset",
    section: SECTION,
    keywords: "note length duration preset",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedNoteLengthPresetStep("next"),
  },
  {
    id: "pianoRoll.length.half",
    title: "Halve selected note lengths",
    section: SECTION,
    keywords: "note length half double duration",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedLengthScale("half"),
  },
  {
    id: "pianoRoll.length.double",
    title: "Double selected note lengths",
    section: SECTION,
    keywords: "note length half double duration",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedLengthScale("double"),
  },
  {
    id: "pianoRoll.start.half",
    title: "Halve selected note starts",
    section: SECTION,
    keywords: "note start half timing ticks",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedNoteStartScale("half"),
  },
  {
    id: "pianoRoll.start.double",
    title: "Double selected note starts",
    section: SECTION,
    keywords: "note start double timing ticks",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedNoteStartScale("double"),
  },
  {
    id: "pianoRoll.key.half",
    title: "Halve selected note distance from C5",
    section: SECTION,
    keywords: "note key C5 distance half pitch",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedNoteKeyScale("half"),
  },
  {
    id: "pianoRoll.key.double",
    title: "Double selected note distance from C5",
    section: SECTION,
    keywords: "note key C5 distance double pitch",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedNoteKeyScale("double"),
  },
  {
    id: "pianoRoll.fromEnd.half",
    title: "Halve selected notes from the end",
    section: SECTION,
    keywords: "note end half length start duration ticks",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedNoteFromEndScale("half"),
  },
  {
    id: "pianoRoll.fromEnd.double",
    title: "Double selected notes from the end",
    section: SECTION,
    keywords: "note end double length start duration ticks",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: () => setSelectedNoteFromEndScale("double"),
  },
  {
    id: "pianoRoll.noteProperties",
    title: "Note properties…",
    section: SECTION,
    defaultShortcut: "Alt+Enter",
    keywords: "velocity pan pitch release modulation start length dialog",
    enabled: hasSelection,
    whyDisabled: () => "Select notes in the piano roll",
    run: openNoteProperties,
  },
  { id: "pianoRoll.noteCurves", title: "Note expression curves…", section: SECTION, keywords: "continuous pan pitch release modulation envelope per note", enabled: hasSelection, whyDisabled: () => "Select source notes", run: openNoteCurves },
  ...TOOL_ACTIONS.map(({ tool, title, key, words }): Action => ({
    id: `pianoRoll.tool${capital(tool)}`,
    title,
    section: SECTION,
    defaultShortcut: key,
    keywords: words,
    enabled: inRoll,
    checked: () => roll().tool === tool,
    run: () => {
      editor()?.cancel()
      roll().setTool(tool)
      currentSession()?.focusGrid()
    },
  })),

  {
    id: "pianoRoll.drum",
    title: "Drum",
    section: SECTION,
    defaultShortcut: "G",
    keywords: "paint toggle drum sequencer steps",
    enabled: inRoll,
    checked: () => roll().drum,
    run: () => {
      editor()?.cancel()
      roll().setDrum(!roll().drum)
      currentSession()?.focusGrid()
    },
  },

  {
    id: "pianoRoll.selectAll",
    title: "Select all notes",
    section: SECTION,
    editCommand: "selectAll",
    defaultShortcut: "Mod+A",
    enabled: hasNotes,
    run: () => editor()?.selectAll(),
  },
  {
    id: "pianoRoll.selectMatchingPitches",
    title: "Select matching pitches",
    section: SECTION,
    enabled: hasSelection,
    run: () => editor()?.selectMatchingPitches(),
  },
  {
    id: "pianoRoll.selectMutedNotes",
    title: "Select muted notes",
    section: SECTION,
    enabled: (state) =>
      inRoll(state) &&
      (editor()?.notes.some((note) => note.velocity === 0) ?? false),
    run: () => editor()?.selectMutedNotes(),
  },
  {
    id: "pianoRoll.restoreMutedNotes",
    title: "Restore muted notes",
    section: SECTION,
    enabled: (state) => inRoll(state) && restorableNotes().length > 0,
    run: async () => {
      const context = editor()?.context
      if (!context) return
      const updates = restorableNotes()
      if (!updates.length) return
      await dispatch({
        type: "updateNotes",
        pattern: context.pattern.id,
        channel: context.channel,
        updates: updates.map(({ id, velocity }) => ({ id, patch: { velocity } })),
      })
    },
  },
  {
    id: "pianoRoll.selectNotesAtPlayhead",
    title: "Select notes at the playhead",
    section: SECTION,
    enabled: (state) => {
      const session = currentSession()
      return (
        inRoll(state) &&
        typeof session?.playhead === "number" &&
        notesAtTick(session.editor.notes, Math.floor(session.playhead)).length > 0
      )
    },
    run: () => {
      const session = currentSession()
      if (!session || typeof session.playhead !== "number") return
      const ids = notesAtTick(session.editor.notes, Math.floor(session.playhead))
      if (ids.length > 0) session.editor.setSelection(ids)
    },
  },
  {
    id: "pianoRoll.deselect",
    title: "Deselect all notes",
    section: SECTION,
    defaultShortcut: "Escape",
    keywords: "clear selection cancel",
    enabled: (state) =>
      inRoll(state) && (roll().selectionCount > 0 || (editor()?.busy ?? false) || (editor()?.canRestoreZoom ?? false)),
    run: () => {
      const current = editor()
      if (!current) return
      // Escape in the middle of a drag gives the drag up instead.
      if (current.busy || current.canRestoreZoom) current.cancel()
      else current.setSelection([])
    },
  },
  {
    id: "pianoRoll.delete",
    title: "Delete selected notes",
    section: SECTION,
    editCommand: "delete",
    defaultShortcut: ["Delete", "Backspace"],
    keywords: "remove erase",
    enabled: hasSelection,
    run: () => editor()?.deleteSelection(),
  },
  {
    id: "pianoRoll.cut",
    title: "Cut notes",
    section: SECTION,
    editCommand: "cut",
    defaultShortcut: "Mod+X",
    enabled: hasSelection,
    run: () => editor()?.cut(),
  },
  {
    id: "pianoRoll.copy",
    title: "Copy notes",
    section: SECTION,
    editCommand: "copy",
    defaultShortcut: "Mod+C",
    enabled: hasSelection,
    run: () => {
      editor()?.copy()
    },
  },
  {
    id: "pianoRoll.paste",
    title: "Paste notes",
    section: SECTION,
    editCommand: "paste",
    defaultShortcut: "Mod+V",
    keywords: "clipboard insert",
    enabled: (state) => inRoll(state) && roll().clipboardCount > 0,
    run: () => {
      const session = currentSession()
      return session?.editor.paste(session.pasteTarget())
    },
  },
  {
    id: "pianoRoll.duplicate",
    title: "Duplicate notes to the right",
    section: SECTION,
    editCommand: "duplicate",
    defaultShortcut: "Mod+D",
    keywords: "repeat clone copy",
    enabled: hasSelection,
    run: () => editor()?.duplicate(),
  },

  {
    id: "pianoRoll.quantize",
    title: "Quantize selected note starts…",
    section: SECTION,
    defaultShortcut: "Mod+Q",
    keywords: "align grid timing",
    enabled: hasSelection,
    whyDisabled: () => "Select notes first",
    run: () => openNoteTools("quantize", "start"),
  },
  {
    id: "pianoRoll.quantizeEnds",
    title: "Quantize selected note ends…",
    section: SECTION,
    defaultShortcut: "Alt+Q",
    keywords: "align grid length",
    enabled: hasSelection,
    whyDisabled: () => "Select notes first",
    run: () => openNoteTools("quantize", "end"),
  },
  ...NOTE_TOOLS.filter((tool) => tool.value !== "quantize").map(
    (tool): Action => ({
      id: `pianoRoll.${tool.value}`,
      ...dynamicTitle("{value0} selected notes…", { value0: tool.label }),
      section: SECTION,
      keywords: tool.description,
      enabled: hasSelection,
      whyDisabled: () => "Select notes first",
      run: () => openNoteTools(tool.value),
    })
  ),

  {
    id: "pianoRoll.nudgeLeft",
    title: "Move notes left by one snap step",
    section: SECTION,
    defaultShortcut: "ArrowLeft",
    repeats: true,
    keywords: "nudge earlier",
    enabled: hasSelection,
    run: () => editor()?.nudge(-1, 0),
  },
  {
    id: "pianoRoll.nudgeRight",
    title: "Move notes right by one snap step",
    section: SECTION,
    defaultShortcut: "ArrowRight",
    repeats: true,
    keywords: "nudge later",
    enabled: hasSelection,
    run: () => editor()?.nudge(1, 0),
  },
  {
    id: "pianoRoll.transposeUp",
    title: "Move notes up a semitone",
    section: SECTION,
    defaultShortcut: "ArrowUp",
    repeats: true,
    keywords: "transpose pitch",
    enabled: hasSelection,
    run: () => editor()?.nudge(0, 1),
  },
  {
    id: "pianoRoll.transposeDown",
    title: "Move notes down a semitone",
    section: SECTION,
    defaultShortcut: "ArrowDown",
    repeats: true,
    keywords: "transpose pitch",
    enabled: hasSelection,
    run: () => editor()?.nudge(0, -1),
  },
  {
    id: "pianoRoll.octaveUp",
    title: "Transpose up an octave",
    section: SECTION,
    defaultShortcut: "Shift+ArrowUp",
    repeats: true,
    keywords: "pitch 12 semitones",
    enabled: hasSelection,
    whyDisabled: () => "Select notes first",
    run: () => editor()?.transpose(12),
  },
  {
    id: "pianoRoll.octaveDown",
    title: "Transpose down an octave",
    section: SECTION,
    defaultShortcut: "Shift+ArrowDown",
    repeats: true,
    keywords: "pitch 12 semitones",
    enabled: hasSelection,
    whyDisabled: () => "Select notes first",
    run: () => editor()?.transpose(-12),
  },

  {
    id: "pianoRoll.zoomFit",
    title: "Zoom to fit all notes",
    section: SECTION,
    defaultShortcut: "Mod+0",
    keywords: "view whole pattern",
    enabled: inRoll,
    run: () => currentSession()?.zoomToFit(),
  },
  {
    id: "pianoRoll.zoomSelection",
    title: "Zoom to the selection",
    section: SECTION,
    defaultShortcut: "Mod+Shift+0",
    keywords: "view fit",
    enabled: hasSelection,
    run: () => currentSession()?.zoomToSelection(),
  },
  {
    id: "pianoRoll.ghosts",
    title: "Ghost notes",
    section: SECTION,
    defaultShortcut: "Alt+G",
    keywords: "show hide other channels",
    checked: () => roll().ghosts,
    run: () => roll().setGhosts(!roll().ghosts),
  },
  {
    id: "pianoRoll.follow",
    title: "Follow the playhead",
    section: SECTION,
    keywords: "scroll auto playback",
    checked: () => roll().follow,
    run: () => roll().setFollow(!roll().follow),
  },

  {
    id: "pianoRoll.editGhosts",
    title: "Edit ghost notes",
    section: SECTION,
    defaultShortcut: "Alt+Shift+G",
    keywords: "other channels source lane ghost editing",
    enabled: inRoll,
    checked: () => roll().editGhosts,
    run: () => roll().setEditGhosts(!roll().editGhosts),
  },

  ...SNAP_OPTIONS.map((option): Action => ({
    id: `pianoRoll.snap.${option.id}`,
    title:
      option.id === "none"
        ? "Turn snap off"
        : `Snap to ${option.label.toLowerCase()}`,
    section: SECTION,
    keywords: "grid magnet",
    checked: () => roll().snap === option.id,
    run: () => roll().setSnap(option.id),
  })),
  ...LANE_KINDS.map((kind): Action => ({
    id: `pianoRoll.lane.${kind.id}`,
    ...dynamicTitle("Show {value0} under the notes", { value0: kind.label.toLowerCase() }),
    section: SECTION,
    keywords: "lane bars",
    checked: () => roll().laneKind === kind.id,
    run: () => roll().setLaneKind(kind.id),
  })),
]

/**
 * Every action here belongs to the piano roll: its keys work while the
 * piano roll has the keyboard, and other panels may use the same keys.
 */
export const PIANO_ROLL_ACTIONS: Action[] = ACTIONS.map((action) => ({
  ...action,
  scope: "pianoRoll",
}))

/**
 * FL Studio's piano roll shortcuts, from the shortcut list in its manual.
 * FL moves the selection with Shift and the arrows; the plain arrows are
 * kept as well. Actions not listed keep their Windfall shortcut. The
 * preset changes keys only: the mouse modifiers are those of
 * `lib/edit-modifiers` in both presets (FL equivalent: Shift+drag clones a
 * note, which here is Ctrl+drag, the same as on the playlist).
 */
export const PIANO_ROLL_FL_KEYMAP: PresetShortcuts = {
  "pianoRoll.toolDraw": ["P"],
  "pianoRoll.toolPaint": ["B"],
  "pianoRoll.toolErase": ["D"],
  "pianoRoll.toolSelect": ["E"],
  "pianoRoll.deselect": ["Mod+D", "Escape"],
  "pianoRoll.duplicate": ["Mod+B"],
  "pianoRoll.nudgeLeft": ["Shift+ArrowLeft", "ArrowLeft"],
  "pianoRoll.nudgeRight": ["Shift+ArrowRight", "ArrowRight"],
  "pianoRoll.transposeUp": ["Shift+ArrowUp", "ArrowUp"],
  "pianoRoll.transposeDown": ["Shift+ArrowDown", "ArrowDown"],
  "pianoRoll.octaveUp": ["Mod+ArrowUp"],
  "pianoRoll.octaveDown": ["Mod+ArrowDown"],
  "pianoRoll.ghosts": ["Alt+V"],
}

/** Adds the piano roll's actions to the registry. Returns a function that removes them. */
export function registerPianoRollActions(): () => void {
  const stops = [
    registry.register(PIANO_ROLL_ACTIONS, {
      presets: { fl: PIANO_ROLL_FL_KEYMAP },
    }),
    invalidateActionsOn(useTypingKeyboardStore, (state) => [state.enabled]),
    invalidateActionsOn(useStepEntryStore, (state) => [state.enabled]),
    invalidateActionsOn(useNoteLogStore, (state) => [state.notes, state.dumping]),
    invalidateActionsOn(usePianoRollStore, (state) => [
      state.tool,
      state.drum,
      state.snap,
      state.ghosts,
      state.follow,
      state.laneKind,
      state.selectionCount,
      state.clipboardCount,
    ]),
  ]
  return () => {
    for (const stop of stops) stop()
  }
}
