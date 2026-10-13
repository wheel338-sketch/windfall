import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import type { Backend } from "@/lib/ipc"
import { startTestApp } from "@/test/harness"
import { useUiStore } from "@/lib/store/ui"
import { ExportDialog } from "./export-dialog"
import { changeExportFormat, exportSampleRates } from "./format-controls"
import { replaceExportExtension } from "./names"

let backend: Backend
let stop: () => void
beforeEach(async () => { ({backend, stop} = await startTestApp()) })
afterEach(() => stop())
function open() { useUiStore.getState().openDialog("export"); render(<ExportDialog />) }

it("disables M4A with the native capability reason", async () => {
  vi.spyOn(backend, "m4aCapability").mockResolvedValue("Windows Media Feature Pack is missing")
  open()
  await screen.findByText(/Windows Media Feature Pack is missing/)
  expect(screen.getByRole("button", {name: "M4A (AAC)"})).toBeDisabled()
})

it("sends M4A bitrate and constrained sample rate through the real export dialog", async () => {
  vi.spyOn(backend, "m4aCapability").mockResolvedValue(null)
  vi.spyOn(backend, "pickExportPath").mockResolvedValue("/exports/Mix.m4a")
  const exported = vi.spyOn(backend, "exportAudio")
  const user = userEvent.setup(); open()
  await waitFor(() => expect(screen.getByRole("button", {name: "M4A (AAC)"})).toBeEnabled())
  await user.click(screen.getByRole("button", {name: "M4A (AAC)"}))
  expect(screen.queryByRole("combobox", {name: "Bit depth"})).toBeNull()
  await user.click(screen.getByRole("combobox", {name: "AAC bitrate"}))
  await user.click(await screen.findByRole("option", {name: "128 kbps"}))
  await user.click(screen.getByRole("button", {name: "Choose…"}))
  await user.click(screen.getByRole("button", {name: "Export"}))
  await waitFor(() => expect(exported).toHaveBeenCalledWith(expect.objectContaining({format: "m4a", aacBitrate: 128, path: "/exports/Mix.m4a"}), expect.stringMatching(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)))
  // The browser backend refuses actual encoding and displays that failure.
  await screen.findByText(/WavPack and M4A audio export require the desktop app/)
  expect(exportSampleRates("m4a")).toEqual([44100, 48000])
})

it("WavPack chooses integer PCM and updates supported filename extensions", async () => {
  const user = userEvent.setup(); open()
  await user.click(screen.getByRole("button", {name: "WavPack"}))
  await user.click(screen.getByRole("combobox", {name: "Bit depth"}))
  expect(screen.queryByRole("option", {name: "32 bit float"})).toBeNull()
  expect(changeExportFormat({format: "wav", bitDepth: "float32", sampleRate: 96000}, "wv"))
    .toEqual({format: "wv", bitDepth: "int24", sampleRate: 96000})
  expect(replaceExportExtension("Mix.wv", "m4a")).toBe("Mix.m4a")
  expect(replaceExportExtension("Mix.M4A", "wv")).toBe("Mix.wv")
})
