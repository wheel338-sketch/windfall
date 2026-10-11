import { describe, expect, it } from "vitest"
import { createMockBackend } from "@/lib/ipc/mock"
import { buildProject, demoProject } from "@/lib/ipc/sim/project"
import { defaultAsset } from "./settings"

describe("sample instrument browser detection", () => {
  it("uses authored transients in shared Rust and saves key maps through undo/redo", async () => {
    const base = demoProject()
    const sample = base.samples[0].id
    let channel = 0
    const project = buildProject(base, (run) => {
      ;[channel] = run({ type: "addChannel", instrument: "sliceDeck" })
      run({ type: "setSamplePlayback", channel, asset: { ...defaultAsset(), sample } })
    })
    const samples = Array<number>(16000).fill(0)
    for (const at of [2000, 6000, 10000]) samples.fill(0.8, at, at + 120)
    const backend = createMockBackend({ storage: null, project, slicerAudio: { [sample]: { sampleRate: 8000, channels: 1, samples } } })
    try {
      const before = await backend.documentSnapshot()
      const guard = await backend.timelineState()
      await backend.samplePlaybackDetect(channel, { mode: "transients", sensitivity: 1 }, guard.generation, guard.revision)
      const after = await backend.documentSnapshot()
      const asset = after.project.channels.find((c) => c.id === channel)!.samplePlayback!
      expect(asset.slices).toHaveLength(4)
      expect(asset.slices.slice(1).map((s) => s.start)).toEqual([0.125, 0.375, 0.625])
      expect(after.history.cursor).toBe(before.history.cursor + 1)
      await backend.undo(); expect((await backend.documentSnapshot()).project).toEqual(before.project)
      await backend.redo(); expect((await backend.documentSnapshot()).project).toEqual(after.project)
      await expect(backend.samplePlaybackDetect(channel, { mode: "grid", gridTicks: 960 }, guard.generation, guard.revision)).rejects.toThrow(/changed/)
    } finally { backend.dispose() }
  })
})
