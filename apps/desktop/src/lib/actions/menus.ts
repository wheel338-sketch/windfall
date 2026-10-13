import { RECENT_SECTION } from "./builtin"
import { translatedText as t } from "@/lib/i18n"

/**
 * Sections the mixer registers one action per kind of effect in, so a kind
 * the core gains shows up in these menus without a change here.
 */
export const ADD_EFFECT_SECTION = "Add effect"
export const REPLACE_EFFECT_SECTION = "Replace effect"

export type MenuEntry =
  /** An action id. */
  | string
  | { separator: true }
  /** A submenu holding every action of a section, such as recent projects. */
  | { submenu: string; section: string; empty: string }
  /** A submenu with entries of its own. */
  | { submenu: string; entries: MenuEntry[] }

export type MenuSpec = { title: string; entries: MenuEntry[] }

const separator = { separator: true } as const

/**
 * The menu bar. Titles, shortcuts and state come from the registry, and
 * every id here must be registered by the time the app has started.
 */
export const MENUS: MenuSpec[] = [
  {
    get title() { return t("File") },
    entries: [
      "file.new",
      "files.newTemplate",
      "file.open",
      "file.importFlp",
      {
        get submenu() { return t("Open recent") },
        section: RECENT_SECTION,
        empty: "No recent projects",
      },
      separator,
      "file.save",
      "file.saveAs",
      "files.saveTemplate",
      "files.saveChannelPreset",
      "files.savePluginPreset",
      "files.loadPreset",
      "files.saveScore",
      "files.loadScore",
      "files.mergeScore",
      "file.saveNewVersion",
      "file.archive",
      "file.cancelArchive",
      separator,
      "file.reloadSamples",
      "file.importMidi",
      separator,
      "file.export",
      "file.exportMidi",
    ],
  },
  {
    get title() { return t("Edit") },
    entries: [
      "edit.undo",
      "edit.redo",
      "edit.history",
      separator,
      "edit.cut",
      "edit.copy",
      "edit.paste",
      "edit.duplicate",
      "edit.delete",
      separator,
      "edit.selectAll",
    ],
  },
  {
    get title() { return t("Add") },
    entries: [
      "plugins.picker",
      "channel.add",
      "channel.addInstrument.subtractiveSynth",
      "channel.addFromFile",
      separator,
      "pattern.add",
      "mixer.addTrack",
      "playlist.addTrack",
      separator,
      "browser.addFolder",
    ],
  },
  {
    get title() { return t("Channels") },
    entries: [
      "channel.rename",
      "channel.color",
      "channel.duplicate",
      "channel.delete",
      separator,
      "channel.replaceSample",
      "channel.initInstrument",
      {
        get submenu() { return t("Synth sounds") },
        section: "Sounds",
        empty: "No sounds",
      },
      separator,
      "channel.mute",
      "channel.solo",
      separator,
      "channel.clearSteps",
      "channel.fill2",
      "channel.fill4",
      "channel.fill8",
      "channel.shiftLeft",
      "channel.shiftRight",
      separator,
      "channel.moveUp",
      "channel.moveDown",
      separator,
      "channel.routeToNewTrack",
      "channel.showInMixer",
    ],
  },
  {
    get title() { return t("Patterns") },
    entries: [
      "pattern.add",
      "pattern.duplicate",
      "pattern.rename",
      "pattern.delete",
      separator,
      "pattern.next",
      "pattern.previous",
      separator,
      "pattern.length16",
      "pattern.length32",
      "pattern.length48",
      "pattern.length64",
    ],
  },
  {
    get title() { return t("Mixer") },
    entries: [
      "mixer.addTrack",
      "mixer.renameTrack",
      "mixer.changeColor",
      "mixer.deleteTrack",
      separator,
      "mixer.toggleMute",
      "mixer.toggleSolo",
      "mixer.unmuteAll",
      "mixer.unsoloAll",
      separator,
      "mixer.resetVolume",
      "mixer.centerPan",
      "mixer.routeToMaster",
      separator,
      "mixer.effects",
      "mixer.enlargeEffects",
      {
        get submenu() { return t("Add effect") },
        section: ADD_EFFECT_SECTION,
        empty: "No effects to add",
      },
      {
        get submenu() { return t("Selected effect") },
        entries: [
          "mixer.openEffect",
          "mixer.bypassEffect",
          separator,
          "mixer.duplicateEffect",
          "mixer.moveEffectUp",
          "mixer.moveEffectDown",
          {
            get submenu() { return t("Replace with") },
            section: REPLACE_EFFECT_SECTION,
            empty: "No effects to replace it with",
          },
          "mixer.resetEffect",
          separator,
          "mixer.removeEffect",
        ],
      },
      separator,
      "mixer.resetPeaks",
    ],
  },
  {
    get title() { return t("View") },
    entries: [
      "view.commandPalette",
      separator,
      "view.browser",
      "view.mixer",
      separator,
      "view.channelRack",
      "view.playlist",
      "view.pianoRoll",
      separator,
      "channelRack.settings",
      {
        get submenu() { return t("Piano roll") },
        entries: [
          "pianoRoll.toolDraw",
          "pianoRoll.toolPaint",
          "pianoRoll.toolSelect",
          "pianoRoll.toolErase",
          separator,
          "pianoRoll.zoomFit",
          "pianoRoll.zoomSelection",
          separator,
          "pianoRoll.ghosts",
          "pianoRoll.follow",
        ],
      },
      {
        get submenu() { return t("Playlist") },
        entries: [
          "playlist.toolDraw",
          "playlist.toolPaint",
          "playlist.toolSelect",
          "playlist.toolErase",
          "playlist.toolMute",
          "playlist.toolSlip",
          "playlist.toolPlayback",
          "playlist.toolSlice",
          separator,
          "playlist.zoomIn",
          "playlist.zoomOut",
          "playlist.zoomToFit",
          separator,
          "playlist.follow",
          "playlist.patterns",
        ],
      },
      separator,
      "view.touchKeyboard",
      "view.touchPads",
      separator,
      "view.toggleTheme",
      "view.resetLayout",
    ],
  },
  {
    get title() { return t("Options") },
    entries: [
      "options.settings",
      "plugins.manage",
      separator,
      "options.keymapWindfall",
      "options.keymapFl",
    ],
  },
  {
    get title() { return t("Help") },
    entries: ["help.gettingStarted", "help.shortcuts", "help.about", "help.assistant", "tools.scripts"],
  },
]

/** Every action id the menus name, submenus included. */
export function menuActionIds(entries: MenuEntry[]): string[] {
  return entries.flatMap((entry) => {
    if (typeof entry === "string") return [entry]
    return "entries" in entry ? menuActionIds(entry.entries) : []
  })
}
