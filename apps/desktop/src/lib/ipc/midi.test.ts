import { describe, expect, it } from "vitest"
import { midiFixture } from "@/features/midi/fixture"
import { DEFAULT_IMPORT, midiExportOptions } from "@/features/midi/options"
import { TEST_DIALOGS } from "@/test/harness"
import { createMockBackend } from "./mock"
import { demoProject } from "./sim/project"

describe("browser MIDI uses the Rust converter", () => {
  it("refuses an import preview when the current project cannot allocate it", async () => {
    const project = demoProject()
    project.nextId = 0xfffffffe
    const backend = createMockBackend({
      storage: null,
      project,
      midiFiles: { "test.mid": midiFixture() },
    })
    try {
      const before = await backend.documentSnapshot()
      await expect(
        backend.midiPreview("test.mid", DEFAULT_IMPORT)
      ).rejects.toThrow("run out of ids")
      expect(await backend.documentSnapshot()).toEqual(before)
    } finally {
      backend.dispose()
    }
  })
  it("appends a reviewed file and undoes it as one step", async () => {
    const backend = createMockBackend({
      storage: null,
      dialogs: TEST_DIALOGS,
      midiFiles: { "test.mid": midiFixture() },
    })
    try {
      const before = await backend.documentSnapshot()
      const preview = await backend.midiPreview("test.mid", DEFAULT_IMPORT)
      expect(preview.notes).toBe(1)
      // Named markers are now imported without loss or adjustment warnings.
      expect(preview.adjustments).toEqual([])
      expect(await backend.documentSnapshot()).toEqual(before)
      await backend.importMidi(preview.token)
      const after = await backend.documentSnapshot()
      expect(after.project.playlist.timeline?.markers).toContainEqual(
        expect.objectContaining({
          tick: 0,
          name: "Verse",
          kind: { type: "named" },
        })
      )
      expect(after.project.channels).toHaveLength(
        before.project.channels.length + 1
      )
      expect(after.history.cursor).toBe(before.history.cursor + 1)
      await backend.undo()
      const restored = await backend.documentSnapshot()
      expect({ ...restored.project, nextId: before.project.nextId }).toEqual(
        before.project
      )
      await backend.redo()
      expect((await backend.documentSnapshot()).project).toEqual(after.project)
      await expect(backend.importMidi(preview.token)).rejects.toThrow("expired")
    } finally {
      backend.dispose()
    }
  })

  it("rejects cancelled, replaced and malformed reviews without editing", async () => {
    const backend = createMockBackend({
      storage: null,
      dialogs: TEST_DIALOGS,
      midiFiles: {
        "good.mid": midiFixture(),
        "bad.mid": new Uint8Array([1, 2]),
      },
    })
    try {
      const preview = await backend.midiPreview("good.mid", DEFAULT_IMPORT)
      await backend.midiDiscard(preview.token)
      await expect(backend.importMidi(preview.token)).rejects.toThrow("expired")
      const before = await backend.documentSnapshot()
      await expect(
        backend.midiPreview("bad.mid", DEFAULT_IMPORT)
      ).rejects.toThrow()
      expect(await backend.documentSnapshot()).toEqual(before)
      const next = await backend.midiPreview("good.mid", DEFAULT_IMPORT)
      await backend.projectNew()
      await expect(backend.importMidi(next.token)).rejects.toThrow("expired")
    } finally {
      backend.dispose()
    }
  })

  it("writes downloadable MIDI bytes, with requested format and resolution", async () => {
    let written: Uint8Array | null = null
    const backend = createMockBackend({
      storage: null,
      dialogs: TEST_DIALOGS,
      midiExport: (_path, bytes) => {
        written = bytes
      },
    })
    try {
      const snapshot = await backend.documentSnapshot()
      const options = {
        ...midiExportOptions("pattern", snapshot.project.patterns[0].id),
        singleTrack: true,
        ppq: 480,
      }
      expect(await backend.exportMidi("beat", options)).toBe("beat.mid")
      expect(written).not.toBeNull()
      expect(Array.from(written!.slice(0, 4))).toEqual([0x4d, 0x54, 0x68, 0x64])
      expect(Array.from(written!.slice(8, 10))).toEqual([0, 0])
      expect(Array.from(written!.slice(12, 14))).toEqual([1, 0xe0])
      expect(await backend.documentSnapshot()).toEqual(snapshot)
      await expect(backend.exportMidi("wrong.wav", options)).rejects.toThrow(
        ".mid"
      )
      await expect(
        backend.exportMidi("bad.mid", { ...options, ppq: 32768 })
      ).rejects.toThrow("resolution")
    } finally {
      backend.dispose()
    }
  })
})
