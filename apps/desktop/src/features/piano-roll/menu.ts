import {
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import { NOTE_COLOR_GROUPS } from "@/lib/note-colors"

const COLOR_GROUPS = ["auto", ...NOTE_COLOR_GROUPS.map((_, group) => String(group))]

export const NOTE_MENU: ContextItem[] = [
  "pianoRoll.dumpPlayedNotes",
  "pianoRoll.exportSheetMusic",
  "pianoRoll.generateRiff",
  "pianoRoll.generateProgression",
  "pianoRoll.noteProperties",
  "pianoRoll.noteCurves",
  { submenu: "Note articulation", items: ["pianoRoll.articulation.normal", "pianoRoll.articulation.slide", "pianoRoll.articulation.portamento"] },
  { submenu: "Set note color / MIDI channel", items: COLOR_GROUPS.map((group) => `pianoRoll.color.${group}`) },
  { submenu: "Select by note color", items: COLOR_GROUPS.map((group) => `pianoRoll.selectColor.${group}`) },
  contextSeparator,
  "pianoRoll.cut",
  "pianoRoll.copy",
  "pianoRoll.paste",
  "pianoRoll.duplicate",
  "pianoRoll.delete",
  contextSeparator,
  "pianoRoll.selectAll",
  "pianoRoll.selectMatchingPitches",
  "pianoRoll.selectMutedNotes",
  "pianoRoll.restoreMutedNotes",
  "pianoRoll.selectNotesAtPlayhead",
  "pianoRoll.deselect",
  contextSeparator,
  "pianoRoll.quantize",
  "pianoRoll.quantizeEnds",
  {
    submenu: "Selected-note tools",
    items: [
      "pianoRoll.legato",
      "pianoRoll.staccato",
      "pianoRoll.portamento",
      "pianoRoll.chop",
      "pianoRoll.chopPattern",
      "pianoRoll.arpeggiate",
      "pianoRoll.flam",
      "pianoRoll.rhythmReshape",
      "pianoRoll.glue",
      "pianoRoll.strum",
      "pianoRoll.flipTime",
      "pianoRoll.flipPitch",
      "pianoRoll.keyRange",
      "pianoRoll.scaleVelocity",
      "pianoRoll.randomize",
      "pianoRoll.generateRandom",
      "pianoRoll.lfo",
    ],
  },
  "pianoRoll.octaveUp",
  "pianoRoll.octaveDown",
  contextSeparator,
  "pianoRoll.zoomSelection",
  "pianoRoll.zoomFit",
]

export const VIEW_MENU: ContextItem[] = [
  "pianoRoll.exportSheetMusic",
  "pianoRoll.patternTimeline",
  "pianoRoll.zoomFit",
  "pianoRoll.zoomSelection",
  contextSeparator,
  "pianoRoll.ghosts",
  "pianoRoll.editGhosts",
  "pianoRoll.waveformHelper",
  "pianoRoll.chordDisplay",
  "pianoRoll.follow",
]

/** The tools, for the parts of the panel around the grid. */
const TOOLS: ContextItem = {
  submenu: "Tool",
  items: [
    "pianoRoll.toolDraw",
    "pianoRoll.toolPaint",
    "pianoRoll.toolSelect",
    "pianoRoll.toolErase",
    "pianoRoll.toolMute",
    "pianoRoll.toolSlice",
    "pianoRoll.toolZoom",
    "pianoRoll.toolPlayback",
  ],
}

/**
 * Everywhere in the piano roll that has no menu of its own: the value
 * lane, its header, the scrollbars and the corners.
 */
export const PANEL_MENU: ContextItem[] = [
  TOOLS,
  contextSeparator,
  "pianoRoll.dumpPlayedNotes",
  "pianoRoll.generateRiff",
  "pianoRoll.generateProgression",
  "pianoRoll.paste",
  "pianoRoll.selectAll",
  "pianoRoll.selectMatchingPitches",
  "pianoRoll.selectMutedNotes",
  "pianoRoll.restoreMutedNotes",
  "pianoRoll.selectNotesAtPlayhead",
  contextSeparator,
  ...VIEW_MENU,
]

/** The piano roll of a project that has no channel to write notes for. */
export const NO_CHANNEL_MENU: ContextItem[] = [
  "channel.add",
  "channel.addFromFile",
  contextSeparator,
  "view.channelRack",
]
