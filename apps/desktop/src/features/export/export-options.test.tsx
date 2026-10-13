import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { Backend } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { settle, startTestApp } from "@/test/harness"

import { ExportDialog } from "./export-dialog"
import { changeExportFormat } from "./format-controls"
import { exportedName, replaceExportExtension } from "./names"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let backend: Backend
let stop: () => void

beforeEach(async () => {
  vi.clearAllMocks()
  ;({ backend, stop } = await startTestApp())
  useUiStore.getState().openDialog("export")
  render(<ExportDialog />)
})
afterEach(() => stop())

async function choose(
  user: ReturnType<typeof userEvent.setup>,
  label: string,
  option: string
) {
  await user.click(screen.getByRole("combobox", { name: label }))
  await user.click(await screen.findByRole("option", { name: option }))
}

async function submit(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "Choose…" }))
  await user.click(screen.getByRole("button", { name: "Export" }))
  await settle()
}

describe("audio export formats", () => {
  it("chooses the selected format in the native save dialog and submitted options", async () => {
    const user = userEvent.setup()
    const picked = vi.spyOn(backend, "pickExportPath")
    const exported = vi.spyOn(backend, "exportAudio")
    await user.click(screen.getByRole("button", { name: "FLAC" }))
    await submit(user)
    expect(picked).toHaveBeenCalledWith(expect.any(String), "flac")
    expect(exported).toHaveBeenCalledWith(
      expect.objectContaining({
        format: "flac",
        path: expect.stringMatching(/\.flac$/),
      }),
      expect.stringMatching(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    )
  })

  it("changes an existing audio extension and keeps FLAC bit depth valid", async () => {
    const user = userEvent.setup()
    await user.type(screen.getByLabelText("Save to"), "/exports/mix.wav")
    await choose(user, "Bit depth", "32 bit float")
    await user.click(screen.getByRole("button", { name: "FLAC" }))
    expect(screen.getByLabelText("Save to")).toHaveValue("/exports/mix.flac")
    expect(
      screen.getByRole("combobox", { name: "Bit depth" })
    ).toHaveTextContent("24 bit")
    await user.click(screen.getByRole("combobox", { name: "Bit depth" }))
    expect(screen.queryByRole("option", { name: "32 bit float" })).toBeNull()
  })

  it("offers valid MP3 rates and switches an unsupported rate to 48 kHz", async () => {
    const user = userEvent.setup()
    await choose(user, "Sample rate", "96 kHz")
    await user.click(screen.getByRole("button", { name: "MP3" }))
    expect(screen.queryByRole("combobox", { name: "Bit depth" })).toBeNull()
    expect(
      screen.getByRole("combobox", { name: "Sample rate" })
    ).toHaveTextContent("48 kHz")
    await user.click(screen.getByRole("combobox", { name: "Sample rate" }))
    expect(screen.queryByRole("option", { name: "96 kHz" })).toBeNull()
  })

  it("submits Vorbis quality and MP3 mode/channel choices", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio")
    await user.click(screen.getByRole("button", { name: "OGG" }))
    await choose(user, "Vorbis quality", "10 (highest)")
    await submit(user)
    expect(exported).toHaveBeenCalledWith(
      expect.objectContaining({ format: "ogg", oggQuality: 10 }),
      expect.stringMatching(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    )
    await backend.exportCancel()
    await settle()
    await user.click(screen.getByRole("button", { name: "MP3" }))
    await choose(user, "Bitrate mode", "Variable bitrate")
    await choose(user, "MP3 quality", "0 (highest)")
    await choose(user, "Channels", "Mono")
    await user.click(screen.getByRole("button", { name: "Export" }))
    await settle()
    expect(exported).toHaveBeenLastCalledWith(
      expect.objectContaining({
        format: "mp3",
        mp3: { rate: { mode: "vbr", quality: 0 }, channels: "mono" },
      }),
      expect.stringMatching(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    )
  })

  it("keeps filenames and format restrictions deterministic", () => {
    expect(replaceExportExtension("C:\\Exports\\Mix.WAV", "ogg")).toBe(
      "C:\\Exports\\Mix.ogg"
    )
    expect(replaceExportExtension("Mix.final", "mp3")).toBe("Mix.final")
    expect(exportedName("/exports/mix", "flac")).toBe("mix.flac")
    expect(
      changeExportFormat(
        { format: "wav", bitDepth: "float32", sampleRate: 96_000 },
        "flac"
      )
    ).toEqual({ format: "flac", bitDepth: "int24", sampleRate: 96_000 })
  })
})

describe("stem exports and cancellation", () => {
  it("submits the stem mode and stops an empty selection before starting export", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio")
    await user.click(
      screen.getByRole("checkbox", { name: "Export mixer tracks as stems" })
    )
    await choose(user, "Each stem contains", "The source through the master")
    await choose(user, "Mixer tracks", "Choose tracks")
    for (const track of useProjectStore
      .getState()
      .project.mixer.tracks.filter((track) => track.id !== 0)) {
      await user.click(screen.getByRole("checkbox", { name: track.name }))
    }
    await user.click(
      screen.getByRole("checkbox", { name: "Include the full mix" })
    )
    await submit(user)
    expect(exported).not.toHaveBeenCalled()
    expect(
      screen.getByText("Choose at least one track or include the full mix.")
    ).toHaveAttribute("role", "alert")
    await user.click(
      screen.getByRole("checkbox", { name: "Include the full mix" })
    )
    await user.click(screen.getByRole("button", { name: "Export" }))
    await settle()
    expect(exported).toHaveBeenCalledWith(
      expect.objectContaining({
        stems: {
          mode: "toMaster",
          tracks: [],
          includeMix: true,
          numbered: true,
          folder: true,
        },
      }),
      expect.stringMatching(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    )
  })

  it("acknowledges cancellation without a success toast and allows another export", async () => {
    const user = userEvent.setup()
    const cancel = vi.spyOn(backend, "exportCancel")
    const exported = vi.spyOn(backend, "exportAudio")
    await submit(user)
    await user.click(screen.getByRole("button", { name: "Cancel export" }))
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Export" })).toBeEnabled()
    )
    expect(cancel).toHaveBeenCalledOnce()
    expect(toast).toHaveBeenCalledWith("Export cancelled")
    expect(toast.success).not.toHaveBeenCalled()
    expect(useUiStore.getState().dialog).toBe("export")
    await user.click(screen.getByRole("button", { name: "Export" }))
    await settle()
    expect(exported).toHaveBeenCalledTimes(2)
  })
})
