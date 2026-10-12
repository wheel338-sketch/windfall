import { afterEach, beforeEach, describe, expect, it } from "vitest"

import { currentKeymap, registry, shortcutLabel } from "@/lib/actions"
import {
  normalizeShortcut,
  resolveChord,
  resolveKeymap,
} from "@/lib/actions/keymap"
import { MENUS, menuActionIds } from "@/lib/actions/menus"
import type { EditCommand, ShortcutScope } from "@/lib/actions/registry"
import { useUiStore, type KeymapPreset } from "@/lib/store/ui"

import { registerAllActions } from "./register-actions"

let unregister: () => void

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true)
  // The way the app starts: the shell's actions, then every panel's.
  unregister = registerAllActions()
})
afterEach(() => unregister())

const everything = () => true
const list = (value: string | string[] | undefined) =>
  value === undefined ? [] : Array.isArray(value) ? value : [value]

describe("the menu bar", () => {
  it("names only actions that are registered once the app has started", () => {
    for (const menu of MENUS) {
      for (const id of menuActionIds(menu.entries)) {
        expect(registry.get(id), `${menu.title} > ${id}`).toBeDefined()
      }
    }
  })

  it("has the menus a DAW is expected to have, with the panels' actions in them", () => {
    expect(MENUS.map((menu) => menu.title)).toEqual([
      "File",
      "Edit",
      "Add",
      "Channels",
      "Patterns",
      "Mixer",
      "View",
      "Options",
      "Help",
    ])
    const ids = (title: string) =>
      menuActionIds(MENUS.find((menu) => menu.title === title)?.entries ?? [])
    expect(ids("File")).toContain("file.reloadSamples")
    expect(ids("Edit")).toEqual([
      "edit.undo",
      "edit.redo",
      "edit.history",
      "edit.cut",
      "edit.copy",
      "edit.paste",
      "edit.duplicate",
      "edit.delete",
      "edit.selectAll",
    ])
    expect(ids("Add")).toEqual([
      "plugins.picker",
      "channel.add",
      "channel.addInstrument.subtractiveSynth",
      "channel.addFromFile",
      "pattern.add",
      "mixer.addTrack",
      "playlist.addTrack",
      "browser.addFolder",
    ])
    expect(ids("View")).toEqual(
      expect.arrayContaining([
        "view.mixer",
        "pianoRoll.toolDraw",
        "playlist.toolMute",
        "pianoRoll.zoomFit",
        "playlist.zoomIn",
      ])
    )
  })

  it("has no action in a menu twice", () => {
    for (const menu of MENUS) {
      const ids = menuActionIds(menu.entries)
      expect(new Set(ids).size, menu.title).toBe(ids.length)
    }
  })
})

describe("the app's keymap", () => {
  it("gives every action the shortcuts it declares, in the Windfall preset", () => {
    const keymap = resolveKeymap(registry.list(), {}, false)
    for (const action of registry.list()) {
      expect(keymap.byAction.get(action.id) ?? [], action.id).toEqual(
        list(action.defaultShortcut)
      )
    }
  })

  it("gives every action the shortcuts the FL preset lists for it", () => {
    const fl = registry.presetShortcuts("fl")
    const keymap = resolveKeymap(registry.list(), fl, false)
    for (const [id, shortcuts] of Object.entries(fl)) {
      expect(registry.get(id), id).toBeDefined()
      expect(keymap.byAction.get(id) ?? [], id).toEqual(shortcuts ?? [])
    }
  })

  it("keeps plain keys inside the panels, so they cannot fire elsewhere", () => {
    for (const preset of ["windfall", "fl"] as KeymapPreset[]) {
      const keymap = resolveKeymap(
        registry.list(),
        registry.presetShortcuts(preset),
        false
      )
      const global = [...(keymap.byScope.get("global")?.keys() ?? [])]
      const plain = global.filter(
        (chord) => !/^(Ctrl|Alt|Meta)\+/.test(chord) && !/^F\d+$/.test(chord)
      )
      // Play, the pattern/song switch, stop preview, and FL's pattern keys.
      expect(plain.sort(), preset).toEqual(
        preset === "windfall"
          ? ["Escape", "L", "Space"]
          : ["Escape", "L", "NumpadAdd", "NumpadSubtract", "Space"]
      )
    }
  })

  function runs(scope: ShortcutScope, shortcut: string): string | undefined {
    const scopes = scope === "global" ? [] : [scope]
    return resolveChord(
      currentKeymap(),
      normalizeShortcut(shortcut, false),
      scopes,
      everything
    )
  }

  it("deletes, duplicates and renames the thing of the panel that has the keyboard", () => {
    expect(runs("channelRack", "Delete")).toBe("channel.delete")
    expect(runs("mixer", "Delete")).toBe("mixer.deleteTrack")
    expect(runs("pianoRoll", "Delete")).toBe("pianoRoll.delete")
    expect(runs("playlist", "Delete")).toBe("playlist.deleteClips")
    expect(runs("browser", "Delete")).toBeUndefined()

    expect(runs("channelRack", "Mod+D")).toBe("channel.duplicate")
    expect(runs("pianoRoll", "Mod+D")).toBe("pianoRoll.duplicate")
    expect(runs("playlist", "Mod+D")).toBe("playlist.duplicate")
    expect(runs("mixer", "Mod+D")).toBeUndefined()
    expect(runs("browser", "Mod+D")).toBeUndefined()

    expect(runs("channelRack", "F2")).toBe("channel.rename")
    expect(runs("mixer", "F2")).toBe("mixer.renameTrack")
    expect(runs("playlist", "F2")).toBe("playlist.renameTrack")
    expect(runs("pianoRoll", "F2")).toBeUndefined()

    expect(runs("pianoRoll", "Mod+A")).toBe("pianoRoll.selectAll")
    expect(runs("playlist", "Mod+A")).toBe("playlist.selectAll")
    expect(runs("pianoRoll", "ArrowLeft")).toBe("pianoRoll.nudgeLeft")
    expect(runs("playlist", "ArrowLeft")).toBe("playlist.nudgeLeft")
    expect(runs("mixer", "ArrowLeft")).toBe("mixer.selectPrevious")
    expect(runs("channelRack", "ArrowLeft")).toBeUndefined()

    // The same letter is a tool in each editor and nothing in the rack.
    expect(runs("pianoRoll", "D")).toBe("pianoRoll.toolDraw")
    expect(runs("playlist", "D")).toBe("playlist.toolDraw")
    expect(runs("channelRack", "D")).toBeUndefined()
    expect(runs("mixer", "M")).toBe("mixer.toggleMute")
    expect(runs("playlist", "M")).toBe("playlist.toolMute")

    for (const scope of [
      "channelRack",
      "mixer",
      "browser",
      "global",
    ] as const) {
      expect(runs(scope, "Space")).toBe("transport.toggle")
    }
  })

  it("renames the pattern with F2 in the FL preset only where no panel has its own", () => {
    useUiStore.getState().setKeymap("fl")
    expect(runs("channelRack", "F2")).toBe("channel.rename")
    expect(runs("mixer", "F2")).toBe("mixer.renameTrack")
    expect(runs("playlist", "F2")).toBe("playlist.renameTrack")
    expect(runs("pianoRoll", "F2")).toBe("pattern.rename")
    expect(runs("browser", "F2")).toBe("pattern.rename")
    expect(shortcutLabel("pattern.rename")).toBe("F2")
    expect(shortcutLabel("channel.rename")).toBe("F2")
  })

  it("uses Ctrl+B for the browser in Windfall and for duplicating in FL's editors", () => {
    for (const scope of ["channelRack", "pianoRoll", "playlist"] as const) {
      expect(runs(scope, "Mod+B")).toBe("view.browser")
    }
    expect(shortcutLabel("view.browser")).toBe("Ctrl+B")

    useUiStore.getState().setKeymap("fl")
    expect(runs("pianoRoll", "Mod+B")).toBe("pianoRoll.duplicate")
    expect(runs("playlist", "Mod+B")).toBe("playlist.duplicate")
    // FL shows its browser with Alt+F8, so the key is free elsewhere.
    expect(runs("channelRack", "Mod+B")).toBeUndefined()
    expect(runs("global", "Alt+F8")).toBe("view.browser")
    expect(shortcutLabel("view.browser")).toBe("Alt+F8")
    expect(shortcutLabel("pianoRoll.duplicate")).toBe("Ctrl+B")
    expect(shortcutLabel("playlist.duplicate")).toBe("Ctrl+B")
    // Ctrl+D deselects in FL's piano roll and still duplicates in the rack.
    expect(runs("pianoRoll", "Mod+D")).toBe("pianoRoll.deselect")
    expect(runs("channelRack", "Mod+D")).toBe("channel.duplicate")
  })
})

describe("the Edit menu's commands", () => {
  it("are declared by the panels that have something to cut, copy or delete", () => {
    const declared = (scope: ShortcutScope) =>
      registry
        .list()
        .filter((action) => action.scope === scope && action.editCommand)
        .map((action) => action.editCommand)
        .sort()
    const all: EditCommand[] = [
      "copy",
      "cut",
      "delete",
      "duplicate",
      "paste",
      "selectAll",
    ]
    expect(declared("pianoRoll")).toEqual(all)
    expect(declared("playlist")).toEqual(all)
    expect(declared("channelRack")).toEqual(["delete", "duplicate"])
    expect(declared("mixer")).toEqual(["delete"])
    expect(declared("browser")).toEqual([])
  })

  it("show the key of the panel that has the keyboard", () => {
    expect(shortcutLabel("edit.delete")).toBe("Del")
    expect(shortcutLabel("edit.duplicate")).toBe("Ctrl+D")
    expect(shortcutLabel("edit.selectAll")).toBeUndefined()

    useUiStore.getState().showCenterTab("playlist")
    expect(shortcutLabel("edit.selectAll")).toBe("Ctrl+A")
    useUiStore.getState().setKeymap("fl")
    expect(shortcutLabel("edit.duplicate")).toBe("Ctrl+B")
  })
})
