import type { Command, DispatchResult, Preset, PresetEntry, PresetReview } from "@/bindings"
import type { ProjectFilesBackend } from "./files"
import { presetFileStem } from "./mixer-presets"
import { sim } from "./sim/wasm"
import type { SimDocument } from "./sim/document"

type Methods = Pick<ProjectFilesBackend, "presetCaptureChannel" | "presetCapturePlugin" | "presetRead" | "presetWrite" | "presetApplyChannel" | "presetApplyPlugin" | "presetLibrary" | "presetLibrarySave" | "presetLibraryDelete" | "pickPresetPath">
/** Real Rust preset parsing/capture/commands; storage alone is in memory.
 * Audio assets and native state remain explicit desktop-only capabilities. */
export function mockPresetFiles(document: () => SimDocument, guard: (generation: number, revision: number) => void, dispatch: (command: Command) => DispatchResult, pickSave: (suggested: string) => Promise<string | null>): Methods {
  const files = new Map<string, string>()
  const library = "/mock/presets/"
  const parse = (text: string) => sim.call<PresetReview>("preset_parse", 0, text)
  function read(path: string) {
    const text = files.get(path)
    if (text === undefined) throw new Error("This preset is not in the browser memory library")
    return parse(text)
  }
  function write(path: string, preset: Preset) {
    const text = JSON.stringify(preset)
    parse(text)
    if (files.has(path)) throw new Error("A preset exists already at that path")
    files.set(path, text)
    return path
  }
  const entry = (path: string) => sim.call<PresetEntry>("preset_entry", 0, { path, preset: read(path).preset })
  return {
    presetCaptureChannel: async (id,generation,revision) => { guard(generation,revision); return document().capturePreset(id,null) },
    presetCapturePlugin: async (target,generation,revision) => { guard(generation,revision); return document().capturePreset(null,target) },
    presetRead: async (path) => read(path),
    presetWrite: async (path,capture) => {
      if (capture.audio) throw new Error("Audio preset files require the desktop app")
      return write(path.toLowerCase().endsWith(".wfpreset") ? path : `${path}.wfpreset`,capture.preset)
    },
    presetApplyChannel: async (path,id,expected,nameColor,_withoutAudio,generation,revision) => {
      guard(generation,revision)
      return dispatch(document().presetCommand(read(path).preset,id,expected,null,nameColor))
    },
    presetApplyPlugin: async (path,target,generation,revision) => {
      guard(generation,revision)
      return dispatch(document().presetCommand(read(path).preset,null,null,target))
    },
    presetLibrary: async () => [...files.keys()].filter((path) => path.startsWith(library)).map(entry).sort((a,b) => a.name.localeCompare(b.name)),
    presetLibrarySave: async (name,capture) => {
      if (capture.audio) throw new Error("Audio preset files require the desktop app")
      if (!name.trim()) throw new Error("Give the preset a name")
      const stem = presetFileStem(name)
      const taken = new Set([...files.keys()].map((path) => path.toLowerCase()))
      for (let number=1;number<=2048;number++) {
        const path = `${library}${stem}${number===1 ? "" : ` ${number}`}.wfpreset`
        if (taken.has(path.toLowerCase())) continue
        write(path,{ ...capture.preset,name: name.trim() }); return entry(path)
      }
      throw new Error("The preset library is full")
    },
    presetLibraryDelete: async (path) => { if (!path.startsWith(library) || !files.delete(path)) throw new Error("This preset is not in the library") },
    pickPresetPath: async (name) => name === undefined ? ([...files.keys()][0] ?? null) : pickSave(`/presets/${presetFileStem(name)}.wfpreset`),
  }
}
