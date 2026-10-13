import type { MidiImportOptions, ProjectPatch } from "@/bindings"
import type { Backend } from "../backend"
import type { SimDocument } from "./document"
import { translatedText as t } from "@/lib/i18n"

export type MockMidiOptions = {
  /** Authored MIDI bytes for headless file and dialog tests. */
  midiFiles?: Record<string, Uint8Array>
  midiFile?: () => Promise<{ name: string; bytes: Uint8Array } | null>
  /** Headless export sink; otherwise exports download a real MIDI file. */
  midiExport?: (path: string, bytes: Uint8Array) => void | Promise<void>
}

function pickFile(): Promise<{ name: string; bytes: Uint8Array } | null> {
  return new Promise((resolve, reject) => {
    const input = document.createElement("input")
    input.type = "file"
    input.accept = ".mid,.midi,audio/midi"
    input.addEventListener("cancel", () => resolve(null), { once: true })
    input.addEventListener(
      "change",
      () => {
        const file = input.files?.[0]
        if (!file) return resolve(null)
        if (file.size > 16 * 1024 * 1024)
          return reject(new Error(t("MIDI files must be at most 16 MiB.")))
        file
          .arrayBuffer()
          .then(
            (buffer) =>
              resolve({ name: file.name, bytes: new Uint8Array(buffer) }),
            reject
          )
      },
      { once: true }
    )
    input.click()
  })
}

export function createMidiMock(
  options: MockMidiOptions,
  document: () => SimDocument,
  publish: (patch: ProjectPatch) => ProjectPatch,
  savePath: (suggested: string) => Promise<string | null>
): Pick<
  Backend,
  | "midiPreview"
  | "importMidi"
  | "midiDiscard"
  | "exportMidi"
  | "pickMidiFile"
  | "pickMidiExportPath"
> {
  const files = new Map(Object.entries(options.midiFiles ?? {}))
  let next = 0
  let pending: {
    token: number
    bytes: number[]
    options: MidiImportOptions
    document: SimDocument
  } | null = null
  return {
    async pickMidiFile() {
      const file = await (options.midiFile ?? pickFile)()
      if (!file) return null
      files.clear()
      files.set(file.name, file.bytes.slice())
      return file.name
    },
    pickMidiExportPath: (name) => savePath(`${name}.mid`),
    async midiPreview(path, importOptions) {
      pending = null
      const file = files.get(path)
      if (!file) throw new Error(t("Choose a MIDI file from your computer first."))
      if (file.length > 16 * 1024 * 1024)
        throw new Error(t("MIDI files must be at most 16 MiB."))
      const bytes = Array.from(file)
      const preview = document().previewMidi(bytes, importOptions)
      const token = ++next
      pending = {
        token,
        bytes,
        options: { ...importOptions },
        document: document(),
      }
      return { ...preview, token }
    },
    async importMidi(token) {
      if (
        !pending ||
        pending.token !== token ||
        pending.document !== document()
      ) {
        throw new Error(t("This MIDI preview has expired. Review the file again."))
      }
      const result = document().importMidi(pending.bytes, pending.options)
      pending = null
      publish(result.patch)
      return result
    },
    async midiDiscard(token) {
      if (pending?.token === token) pending = null
    },
    async exportMidi(path, exportOptions) {
      const clean = path.trim()
      if (!clean) throw new Error(t("Choose where to save the MIDI file."))
      const name = /\.[^/\\]+$/.test(clean) ? clean : `${clean}.mid`
      if (!/\.midi?$/i.test(name))
        throw new Error(t("Choose a .mid or .midi file for MIDI export."))
      const bytes = new Uint8Array(document().exportMidi(exportOptions))
      if (options.midiExport) await options.midiExport(name, bytes)
      else {
        const url = URL.createObjectURL(
          new Blob([bytes], { type: "audio/midi" })
        )
        const anchor = window.document.createElement("a")
        anchor.href = url
        anchor.download = name.split(/[\\/]/).at(-1) ?? name
        anchor.click()
        setTimeout(() => URL.revokeObjectURL(url), 1000)
      }
      return name
    },
  }
}
