import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { startTestApp } from "@/test/harness"
import type { MockBackend } from "@/lib/ipc/mock"
import { useProjectStore } from "@/lib/store/project"
import { StemSeparationSettings } from "./stem-separation"
let backend: MockBackend
let stop: () => void
beforeEach(async () => { ({ backend, stop } = await startTestApp()) })
afterEach(() => stop())
it("chooses a runtime and registered model, saves settings and preserves the simulator project/history", async () => {
  const settings = { libraryPath: null as string | null, modelId: "licensed-pack", modelVersion: "1" }
  vi.spyOn(backend, "analysisRuntimeSettings").mockResolvedValue(settings)
  vi.spyOn(backend, "analysisCapability").mockResolvedValue({ native: true, available: true, stemSeparation: true, reason: null,
    models: [{ id: "licensed-pack", version: "1", revision: "1", sha256: "01".repeat(32), bytes: "64", maxBytes: "128",
      sampleRate: 44100, inputChannels: 2, maxInputFrames: "44100", outputs: ["drums", "bass", "vocals", "other"].map((role) => ({ role, channels: 2 })),
      provenance: { origin: "authored-test", sourceRevision: "1", author: "fixture", licenseSpdx: "CC0-1.0", licenseReference: "fixture",
        adapterId: "windfall-onnx-separation", adapterVersion: "1", device: "cpu" } }] })
  vi.spyOn(backend, "pickAnalysisRuntime").mockResolvedValue("C:\\runtime\\onnxruntime.dll")
  const configure = vi.spyOn(backend, "analysisRuntimeConfigure").mockImplementation(async (s) => s)
  const before = useProjectStore.getState()
  render(<StemSeparationSettings />)
  await screen.findByRole("option", { name: "licensed-pack 1" })
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "Choose runtime library" })))
  expect(screen.getByLabelText("ONNX Runtime library")).toHaveValue("C:\\runtime\\onnxruntime.dll")
  fireEvent.click(screen.getByRole("button", { name: "Save separation settings" }))
  await waitFor(() => expect(configure).toHaveBeenCalledWith({ ...settings, libraryPath: "C:\\runtime\\onnxruntime.dll" }))
  expect(useProjectStore.getState()).toBe(before)
})
it("shows an actual capability refusal and disables installation controls in browser mode", async () => {
  const before = useProjectStore.getState()
  render(<StemSeparationSettings />)
  expect(await screen.findByRole("status")).toHaveTextContent("No inference runs in the browser")
  expect(screen.getByRole("button", { name: "Save separation settings" })).toBeDisabled()
  expect(useProjectStore.getState()).toBe(before)
})
