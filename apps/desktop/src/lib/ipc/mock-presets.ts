import type { Command, DispatchResult, Preset, PresetEntry, PresetReview } from "@/bindings"
import type { ProjectFilesBackend } from "./files"
import { presetFileStem } from "./mixer-presets"
import { sim } from "./sim/wasm"
import type { SimDocument } from "./sim/document"
import { translatedText as t, message } from "@/lib/i18n"

type Methods = Pick<ProjectFilesBackend, "presetCaptureChannel" | "presetCapturePlugin" | "presetRead" | "presetWrite" | "presetApplyChannel" | "presetApplyPlugin" | "presetLibrary" | "presetLibrarySave" | "presetLibraryDelete" | "pickPresetPath">
/** Real Rust preset parsing/capture/commands; storage alone is in memory.
 * Audio assets and native state remain explicit desktop-only capabilities. */
export function mockPresetFiles(document: () => SimDocument, guard: (generation: number, revision: number) => void, dispatch: (command: Command) => DispatchResult, pickSave: (suggested: string) => Promise<string | null>): Methods {
  const files = new Map<string, string>()
  const library = "/mock/presets/"
  const parse = (text: string) => sim.call<PresetReview>("preset_parse", 0, text)
  function read(path: string) {
    const text = files.get(path)
    if (text === undefined) throw new Error(t("This preset is not in the browser memory library"))
    return { ...parse(text), path }
  }
  function write(path: string, preset: Preset) {
    const text = JSON.stringify(preset)
    parse(text)
    files.set(path, text)
    return path
  }
  const entry = (path: string) => sim.call<PresetEntry>("preset_entry", 0, { path, preset: read(path).preset })
  return {
    presetCaptureChannel: async (id,generation,revision) => { guard(generation,revision); return document().capturePreset(id,null) },
    presetCapturePlugin: async (target,generation,revision) => { guard(generation,revision); return document().capturePreset(null,target) },
    presetRead: async (path) => read(path),
    presetWrite: async (path,capture) => {
      if (capture.audio) throw new Error(t("Audio preset files require the desktop app"))
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
      if (capture.audio) throw new Error(t("Audio preset files require the desktop app"))
      if (!name.trim()) throw new Error(t("Give the preset a name"))
      // Mirror session/presets.rs: library saves replace this safe name.
      const stem = [...name.trim().replace(/[<>:"/\\|?*\p{Cc}]/gu, "_")].slice(0,120).join("").replace(/[. ]+$/g, "").trimStart()
      if (!stem || stem === "." || stem === ".." || /^(CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])$/i.test(stem)) throw new Error(message("\"{name}\" cannot be used as a file name.", { name }))
      let displayName = name.replace(/\p{Cc}/gu, "").trim()
      while (new TextEncoder().encode(displayName).length > 256) displayName = [...displayName].slice(0,-1).join("")
      const path = `${library}${stem}.wfpreset`
      write(path,{ ...capture.preset,name: displayName.trim() }); return entry(path)
    },
    presetLibraryDelete: async (path) => { if (!path.startsWith(library) || !files.delete(path)) throw new Error(t("This preset is not in the library")) },
    pickPresetPath: async (name) => name === undefined ? ([...files.keys()][0] ?? null) : pickSave(`/presets/${presetFileStem(name)}.wfpreset`),
  }
}
