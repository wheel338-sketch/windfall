import type { MixerTrackPreset } from "@/bindings"

/*
 * The user's mixer track preset library: a folder of `.wfmixer` files the
 * app shell keeps beside its settings. These types mirror the structs of
 * `session/mixer_presets.rs`, which the shell sends as plain JSON.
 */

/** A hosted plugin a preset refers to. The plugin is not in the file. */
export type MixerPresetPluginRef = {
  format: string
  id: string
  name: string
  path: string
}

/** One file of the library, as the preset browser lists it. */
export type MixerPresetLibraryEntry = {
  /** The file's name in the library folder: what read and delete take. */
  file: string
  /** The full path, for display. */
  path: string
  /** The preset's name, or the file's when the file cannot be read. */
  name: string
  effects: number
  plugins: MixerPresetPluginRef[]
  bytes: number
  /** Milliseconds since the Unix epoch, where the file system says. */
  modified: number | null
  /** Why this file cannot be applied. The other files are unaffected. */
  error: string | null
}

export type MixerPresetLibrary = {
  folder: string
  entries: MixerPresetLibraryEntry[]
  /** The folder holds more presets than are listed. */
  truncated: boolean
}

export interface MixerPresetLibraryBackend {
  /** Lists the library. A damaged file is listed with its error. */
  mixerPresetLibrary(): Promise<MixerPresetLibrary>
  /** Saves under a name made from the preset's; never overwrites. */
  mixerPresetLibrarySave(
    preset: MixerTrackPreset
  ): Promise<MixerPresetLibraryEntry>
  mixerPresetLibraryRead(file: string): Promise<MixerTrackPreset>
  mixerPresetLibraryDelete(file: string): Promise<void>
}

export const MIXER_PRESET_EXTENSION = "wfmixer"
export const MAX_LIBRARY_PRESETS = 512
const MAX_FILE_STEM_CHARS = 96

/** The file name a preset of this name is saved under, without extension. */
export function presetFileStem(name: string): string {
  const cleaned = [...name]
    // eslint-disable-next-line no-control-regex
    .map((char) => (/[\u0000-\u001f\u007f<>:"/\\|?*]/.test(char) ? "_" : char))
    .slice(0, MAX_FILE_STEM_CHARS)
    .join("")
    .replace(/^[.\s]+|[.\s]+$/g, "")
  const device = (cleaned.split(".")[0] ?? "").toUpperCase()
  const reserved =
    ["CON", "PRN", "AUX", "NUL"].includes(device) ||
    /^(COM|LPT)[0-9]$/.test(device)
  if (cleaned === "") return "Mixer track"
  return reserved ? `_${cleaned}` : cleaned
}

function looksLikePreset(value: unknown): value is MixerTrackPreset {
  const preset = value as Partial<MixerTrackPreset> | null
  return (
    typeof preset === "object" &&
    preset !== null &&
    typeof preset.name === "string" &&
    Array.isArray(preset.effects) &&
    Array.isArray(preset.plugins)
  )
}

/**
 * A library kept in memory, for the browser build and the tests. It names,
 * numbers, lists and refuses the way the desktop shell does, without a
 * file system behind it. `files` can be handed in to start from, or to put
 * a damaged file in the listing.
 */
export function mockMixerPresetLibrary(
  files: Map<string, string> = new Map()
): MixerPresetLibraryBackend {
  const folder = "/mock/mixer-presets"
  const isPresetFile = (file: string) =>
    file.toLowerCase().endsWith(`.${MIXER_PRESET_EXTENSION}`) &&
    !/[\\/:]/.test(file) &&
    file !== `.${MIXER_PRESET_EXTENSION}`

  function parse(text: string): MixerTrackPreset {
    let value: unknown
    try {
      value = JSON.parse(text)
    } catch (error) {
      throw new Error(
        `Invalid mixer preset: ${error instanceof Error ? error.message : String(error)}`,
        { cause: error }
      )
    }
    if (!looksLikePreset(value)) throw new Error("Invalid mixer preset")
    if (value.version !== 1) throw new Error("Unsupported mixer preset version")
    return value
  }

  function describe(file: string): MixerPresetLibraryEntry {
    const text = files.get(file) ?? ""
    const entry = {
      file,
      path: `${folder}/${file}`,
      bytes: new TextEncoder().encode(text).length,
      modified: null,
    }
    try {
      const preset = parse(text)
      return {
        ...entry,
        name: preset.name,
        effects: preset.effects.length,
        plugins: preset.plugins.map(({ format, id, name, path }) => ({
          format,
          id,
          name,
          path,
        })),
        error: null,
      }
    } catch (error) {
      return {
        ...entry,
        name: file.replace(/\.[^.]*$/, ""),
        effects: 0,
        plugins: [],
        error: error instanceof Error ? error.message : String(error),
      }
    }
  }

  function known(file: string): string {
    if (!isPresetFile(file))
      throw new Error("That is not a preset of the mixer preset library")
    return file
  }

  return {
    mixerPresetLibrary: async () => {
      const names = [...files.keys()]
        .filter(isPresetFile)
        .sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()))
      const listed = names.slice(0, MAX_LIBRARY_PRESETS).map(describe)
      listed.sort(
        (a, b) =>
          a.name.toLowerCase().localeCompare(b.name.toLowerCase()) ||
          a.file.toLowerCase().localeCompare(b.file.toLowerCase())
      )
      return {
        folder,
        entries: listed,
        truncated: names.length > MAX_LIBRARY_PRESETS,
      }
    },
    mixerPresetLibrarySave: async (preset) => {
      if (!looksLikePreset(preset) || preset.version !== 1)
        throw new Error("Unsupported mixer preset version")
      if ([...files.keys()].filter(isPresetFile).length >= MAX_LIBRARY_PRESETS)
        throw new Error(
          `The mixer preset library is full (${MAX_LIBRARY_PRESETS} presets). Delete one first`
        )
      const stem = presetFileStem(preset.name)
      const taken = new Set([...files.keys()].map((file) => file.toLowerCase()))
      for (let number = 1; number <= MAX_LIBRARY_PRESETS + 1; number += 1) {
        const file =
          number === 1
            ? `${stem}.${MIXER_PRESET_EXTENSION}`
            : `${stem} ${number}.${MIXER_PRESET_EXTENSION}`
        if (taken.has(file.toLowerCase())) continue
        files.set(file, JSON.stringify(preset, null, 2))
        return describe(file)
      }
      throw new Error("Too many presets of the library share this name")
    },
    mixerPresetLibraryRead: async (file) => {
      const text = files.get(known(file))
      if (text === undefined)
        throw new Error("Could not open the mixer preset: it is not there")
      return structuredClone(parse(text))
    },
    mixerPresetLibraryDelete: async (file) => {
      if (!files.delete(known(file)))
        throw new Error("That preset is no longer in the library")
    },
  }
}
