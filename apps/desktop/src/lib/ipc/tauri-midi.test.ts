import { beforeEach, describe, expect, it, vi } from "vitest"
import { invoke } from "@tauri-apps/api/core"
import { open, save } from "@tauri-apps/plugin-dialog"
import { DEFAULT_IMPORT, midiExportOptions } from "@/features/midi/options"
import { createTauriBackend } from "./tauri"

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), Channel: class {} }))
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }))
beforeEach(() => vi.clearAllMocks())

describe("native MIDI IPC and dialogs", () => {
  it("uses MIDI filters and treats cancellation as null", async () => {
    vi.mocked(open).mockResolvedValue(null)
    vi.mocked(save).mockResolvedValue("C:/song.mid")
    const backend = createTauriBackend()
    expect(await backend.pickMidiFile()).toBeNull()
    expect(open).toHaveBeenCalledWith(
      expect.objectContaining({
        multiple: false,
        filters: [{ name: "MIDI file", extensions: ["mid", "midi"] }],
      })
    )
    expect(await backend.pickMidiExportPath("Song")).toBe("C:/song.mid")
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({
        defaultPath: "Song.mid",
        filters: [{ name: "MIDI file", extensions: ["mid", "midi"] }],
      })
    )
  })

  it("passes review tokens and options and converts shell errors to Error", async () => {
    const backend = createTauriBackend()
    const caller = { generation: 1, token: "native-midi-window" }
    vi.mocked(invoke).mockResolvedValueOnce({ generation: 1, caller, snapshot: {} }).mockResolvedValue(null)
    await backend.documentSnapshot()
    await backend.midiPreview("C:/song.mid", DEFAULT_IMPORT)
    expect(invoke).toHaveBeenLastCalledWith("midi_preview", {
      path: "C:/song.mid",
      options: DEFAULT_IMPORT,
      caller,
    })
    await backend.importMidi(7)
    expect(invoke).toHaveBeenLastCalledWith("import_midi", { token: 7, caller })
    await backend.midiDiscard(7)
    expect(invoke).toHaveBeenLastCalledWith("midi_discard", { token: 7, caller })
    const options = midiExportOptions("song", 1)
    await backend.exportMidi("C:/song.mid", options)
    expect(invoke).toHaveBeenLastCalledWith("export_midi", {
      path: "C:/song.mid",
      options,
      caller,
    })
    vi.mocked(invoke).mockRejectedValue("the MIDI track is truncated")
    await expect(
      backend.midiPreview("broken.mid", DEFAULT_IMPORT)
    ).rejects.toThrow("The MIDI track is truncated")
  })
})
