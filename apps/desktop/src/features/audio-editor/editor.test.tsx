import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { startTestApp } from "@/test/harness"
import type { MockBackend } from "@/lib/ipc/mock"
import { useProjectStore } from "@/lib/store/project"
import { usePlaylistStore } from "@/features/playlist/store"
import { AudioEditorButton } from "./index"
import type { AudioEditPreview } from "./types"
import { Waveform } from "./waveform"

const preview: AudioEditPreview = {
  token: 7,
  clip: 42,
  name: "Take",
  frames: 1000,
  sampleRate: 1000,
  peaks: [-0.5, 0.5, -0.2, 0.2],
}
let backend: MockBackend
let stop: () => void
beforeEach(async () => {
  ;({ backend, stop } = await startTestApp())
})
afterEach(() => stop())

async function open() {
  const view = render(<AudioEditorButton clip={42} />)
  fireEvent.click(screen.getByRole("button", { name: "Audio editor" }))
  await screen.findByRole("img", { name: /Clip waveform/ })
  return view
}

describe("selected clip audio editor", () => {
  it("exports the annotated WAV selection and shows native timeline capability errors", async () => {
    vi.spyOn(backend, "audioEditorOpen").mockResolvedValue({ ...preview, regions: [
      { name: "Phrase", startFrame: 100, endFrame: 500, notes: ["Authored cue note", "WindfallNavigation/1 skip 500"] },
    ] })
    vi.spyOn(backend, "pickExportPath").mockResolvedValue("/exports/Take.wav")
    const saved = vi.spyOn(backend, "audioEditorSaveWav")
    const imported = vi.spyOn(backend, "audioEditorImportTimeline")
    const before = useProjectStore.getState().project
    await open()
    expect(screen.getByLabelText("Notes of annotation 1")).toHaveValue("Authored cue note\nWindfallNavigation/1 skip 500")
    fireEvent.change(screen.getByLabelText("Start frame (included)"), { target: { value: "100" } })
    fireEvent.change(screen.getByLabelText("End frame (excluded)"), { target: { value: "500" } })
    fireEvent.click(screen.getByRole("button", { name: "Save selection as WAV with annotations" }))
    await waitFor(() => expect(saved).toHaveBeenCalledWith(7, 100, 500, "/exports/Take.wav"))
    await screen.findByText("WAV saving requires the desktop app.")
    fireEvent.click(screen.getByRole("button", { name: "Import annotations to timeline" }))
    await waitFor(() => expect(imported).toHaveBeenCalledWith(7))
    await screen.findByText("WAV timeline import requires the desktop app.")
    expect(useProjectStore.getState().project).toEqual(before)
  })
  it("sends the editor selection to WavPack saving and displays a capability failure", async () => {
    vi.spyOn(backend, "audioEditorOpen").mockResolvedValue(preview)
    vi.spyOn(backend, "pickExportPath").mockResolvedValue("/exports/Take.wv")
    const saved = vi.spyOn(backend, "audioEditorSaveWavpack")
    await open()
    fireEvent.change(screen.getByLabelText("Start frame (included)"), {target: {value: "100"}})
    fireEvent.change(screen.getByLabelText("End frame (excluded)"), {target: {value: "900"}})
    fireEvent.click(screen.getByRole("button", {name: "Save selection as WavPack"}))
    await waitFor(() => expect(saved).toHaveBeenCalledWith(7, 100, 900, "/exports/Take.wv", "int24"))
    await screen.findByText("WavPack saving requires the desktop app.")
    expect(backend.pickExportPath).toHaveBeenCalledWith("Take", "wv")
  })
  it("cancelled WavPack save dialog does not publish or edit the project", async () => {
    vi.spyOn(backend, "audioEditorOpen").mockResolvedValue(preview)
    vi.spyOn(backend, "pickExportPath").mockResolvedValue(null)
    const saved = vi.spyOn(backend, "audioEditorSaveWavpack")
    const before = useProjectStore.getState().project
    await open()
    fireEvent.click(screen.getByRole("button", {name: "Save selection as WavPack"}))
    await waitFor(() => expect(screen.getByRole("button", {name: "Save selection as WavPack"})).toBeEnabled())
    expect(saved).not.toHaveBeenCalled()
    expect(useProjectStore.getState().project).toEqual(before)
  })
  it.each(["project replacement", "unmount"])(
    "ignores an Apply reply after %s, even with reused revisions",
    async (change) => {
      vi.spyOn(backend, "audioEditorOpen").mockResolvedValue(preview)
      const result = await backend.addAudioClipFromFile(
        "/factory/Loops/Drum loop 128.wav",
        { start: 0 }
      )
      await backend.projectNew()
      let resolve!: (value: typeof result) => void
      vi.spyOn(backend, "audioEditorApply").mockImplementation(
        () =>
          new Promise((done) => {
            resolve = done
          })
      )
      const view = await open()
      fireEvent.click(screen.getByRole("button", { name: "Normalize" }))
      if (change === "project replacement")
        await act(() => backend.projectNew())
      else view.unmount()
      const before = useProjectStore.getState()
      const selection = new Set(usePlaylistStore.getState().selection)
      expect(before.revision).toBe(0)
      expect(result.patch.revision).toBe(1)
      await act(async () => resolve(result))
      expect(useProjectStore.getState()).toBe(before)
      expect(usePlaylistStore.getState().selection).toEqual(selection)
    }
  )
  it("applies a native result patch, stays open on the derived clip and renews its native token", async () => {
    const opened = vi.spyOn(backend, "audioEditorOpen").mockResolvedValueOnce(preview)
    // The result fixture exercises patch/selection delivery; the real WAV and
    // atomic native history path are covered by Rust session fixtures.
    const result = await backend.addAudioClipFromFile(
      "/factory/Loops/Drum loop 128.wav",
      { start: 0 }
    )
    opened.mockResolvedValue({ ...preview, token: 8, clip: result.created.at(-1)! })
    vi.spyOn(backend, "audioEditorApply").mockResolvedValue(result)
    await open()
    fireEvent.click(screen.getByRole("button", { name: "Normalize" }))
    await waitFor(() => expect(backend.audioEditorOpen).toHaveBeenCalledWith(result.created.at(-1)))
    expect(screen.getByRole("dialog")).toBeVisible()
    expect(usePlaylistStore.getState().selection).toEqual(
      new Set([result.created.at(-1)])
    )
    expect(useProjectStore.getState().revision).toBe(result.patch.revision)
    expect(useProjectStore.getState().history.cursor).toBe(1)
    await waitFor(() => expect(screen.getByRole("button", { name: "Normalize" })).toBeEnabled())
    const refused = vi.spyOn(backend, "audioEditorApply").mockRejectedValue(new Error("second operation refused"))
    fireEvent.click(screen.getByRole("button", { name: "Reverse selection" }))
    await waitFor(() => expect(refused).toHaveBeenLastCalledWith(expect.objectContaining({ token: 8 })))
  })
  it("follows its edit to the derived clip when the playlist drops the replaced clip before the reply", async () => {
    const opened = vi.spyOn(backend, "audioEditorOpen").mockResolvedValueOnce(preview)
    const result = await backend.addAudioClipFromFile(
      "/factory/Loops/Drum loop 128.wav",
      { start: 0 }
    )
    const derived = result.created.at(-1)!
    opened.mockResolvedValue({ ...preview, token: 8, clip: derived })
    let resolve!: (value: typeof result) => void
    vi.spyOn(backend, "audioEditorApply").mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done
        })
    )
    act(() => usePlaylistStore.getState().select([preview.clip]))
    await open()
    fireEvent.click(screen.getByRole("button", { name: "Fade out selection" }))
    // The replacement arrives first, and the playlist forgets the removed clip.
    act(() => usePlaylistStore.getState().clearSelection())
    await act(async () => resolve(result))
    await waitFor(() => expect(opened).toHaveBeenLastCalledWith(derived))
    expect(usePlaylistStore.getState().selection).toEqual(new Set([derived]))
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Fade out selection" })).toBeEnabled()
    )
    expect(screen.queryByText("Project or source changed. Reopen the editor.")).toBeNull()
    expect(screen.queryByText("The project changed. Close and reopen the audio editor.")).toBeNull()
  })
  it("explicitly refuses browser file operations and preserves document/history", async () => {
    const before = useProjectStore.getState()
    render(<AudioEditorButton clip={42} />)
    fireEvent.click(screen.getByRole("button", { name: "Audio editor" }))
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "requires the desktop app"
    )
    expect(screen.queryByRole("img")).toBeNull()
    await expect(
      backend.audioEditorApply({
        token: 1,
        operation: "reverse",
        startFrame: 0,
        endFrame: 1,
      })
    ).rejects.toThrow("No audio or project changes")
    expect(useProjectStore.getState()).toBe(before)
  })
  it("sends exact frame endpoints and native token for an operation, shows stale errors", async () => {
    vi.spyOn(backend, "audioEditorOpen").mockResolvedValue(preview)
    const apply = vi
      .spyOn(backend, "audioEditorApply")
      .mockRejectedValue(
        new Error(
          "The project or source changed. Reopen the audio editor and try again."
        )
      )
    await open()
    expect(screen.getByRole("button", { name: "Cut selection" })).toBeDisabled()
    fireEvent.change(screen.getByLabelText("Start frame (included)"), {
      target: { value: "100" },
    })
    fireEvent.change(screen.getByLabelText("End frame (excluded)"), {
      target: { value: "900" },
    })
    fireEvent.click(screen.getByRole("button", { name: "Reverse selection" }))
    await waitFor(() =>
      expect(apply).toHaveBeenCalledWith({
        token: 7,
        operation: "reverse",
        startFrame: 100,
        endFrame: 900,
      })
    )
    expect(await screen.findByRole("alert")).toHaveTextContent("source changed")
    expect(screen.getByRole("img")).toHaveAccessibleName(
      "Clip waveform, selected frames 100 to 900"
    )
  })
  it("keeps invalid ranges unapplied and resets selection to all", async () => {
    vi.spyOn(backend, "audioEditorOpen").mockResolvedValue(preview)
    const apply = vi.spyOn(backend, "audioEditorApply")
    await open()
    fireEvent.change(screen.getByLabelText("Start frame (included)"), {
      target: { value: "1000" },
    })
    expect(screen.getByRole("button", { name: "Normalize" })).toBeDisabled()
    expect(screen.getByLabelText("Start frame (included)")).toHaveAttribute(
      "aria-invalid",
      "true"
    )
    expect(apply).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole("button", { name: "Select all" }))
    expect(screen.getByLabelText("Start frame (included)")).toHaveValue(0)
    expect(screen.getByLabelText("End frame (excluded)")).toHaveValue(1000)
    expect(screen.getByRole("button", { name: "Normalize" })).toBeEnabled()
  })
  it("releases the rendered view on close and prevents duplicate clicks/close during apply", async () => {
    vi.spyOn(backend, "audioEditorOpen").mockResolvedValue(preview)
    const discard = vi.spyOn(backend, "audioEditorDiscard")
    let reject!: (error: Error) => void
    const apply = vi.spyOn(backend, "audioEditorApply").mockImplementation(
      () =>
        new Promise((_, no) => {
          reject = no
        })
    )
    await open()
    fireEvent.click(screen.getByRole("button", { name: "Normalize" }))
    expect(screen.getByRole("button", { name: "Normalize" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Close editor" })).toBeDisabled()
    fireEvent.click(screen.getByRole("button", { name: "Normalize" }))
    expect(apply).toHaveBeenCalledTimes(1)
    await act(async () => reject(new Error("Write failed")))
    fireEvent.click(screen.getByRole("button", { name: "Close editor" }))
    await waitFor(() => expect(discard).toHaveBeenCalledWith(7))
  })
  it("discards a view that finishes opening after its dialog was closed", async () => {
    let resolve!: (preview: AudioEditPreview) => void
    vi.spyOn(backend, "audioEditorOpen").mockImplementation(
      () =>
        new Promise((yes) => {
          resolve = yes
        })
    )
    const discard = vi.spyOn(backend, "audioEditorDiscard")
    render(<AudioEditorButton clip={42} />)
    fireEvent.click(screen.getByRole("button", { name: "Audio editor" }))
    fireEvent.click(screen.getByRole("button", { name: "Close editor" }))
    await act(async () => resolve(preview))
    await waitFor(() => expect(discard).toHaveBeenCalledWith(7))
  })
  it("discards a view opened against a replaced project at the same revision", async () => {
    let resolve!: (value: AudioEditPreview) => void
    vi.spyOn(backend, "audioEditorOpen").mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done
        })
    )
    const discard = vi.spyOn(backend, "audioEditorDiscard")
    render(<AudioEditorButton clip={42} />)
    fireEvent.click(screen.getByRole("button", { name: "Audio editor" }))
    await act(() => backend.projectNew())
    const before = useProjectStore.getState()
    await act(async () => resolve(preview))
    expect(discard).toHaveBeenCalledWith(preview.token)
    expect(screen.queryByRole("img")).toBeNull()
    expect(screen.getByRole("alert")).toHaveTextContent("The project changed")
    expect(useProjectStore.getState()).toBe(before)
  })
  it("selects the same frame range when dragging backwards on the waveform", () => {
    const onSelection = vi.fn()
    render(
      <Waveform
        preview={preview}
        selection={{ start: 0, end: 1000 }}
        onSelection={onSelection}
        disabled={false}
      />
    )
    const waveform = screen.getByRole("img")
    vi.spyOn(waveform, "getBoundingClientRect").mockReturnValue({
      left: 0,
      width: 1000,
    } as DOMRect)
    waveform.setPointerCapture = vi.fn()
    waveform.releasePointerCapture = vi.fn()
    fireEvent.pointerDown(waveform, { button: 0, pointerId: 1, clientX: 900 })
    fireEvent.pointerMove(waveform, { pointerId: 1, clientX: 100 })
    fireEvent.pointerUp(waveform, { pointerId: 1, clientX: 100 })
    expect(onSelection).toHaveBeenLastCalledWith({ start: 100, end: 900 })
  })
  it("reads the range as time and labels the waveform's time and the spectrogram's frequency", async () => {
    vi.spyOn(backend, "audioEditorOpen").mockResolvedValue(preview)
    await open()
    fireEvent.change(screen.getByLabelText("Start frame (included)"), { target: { value: "250" } })
    expect(screen.getByText("0:00.250 from the clip start")).toBeInTheDocument()
    expect(screen.getByText("0:01.000 from the clip start")).toBeInTheDocument()
    // A one-second clip is marked every 0.2 s.
    expect(screen.getByText("0:00.4")).toBeInTheDocument()
    render(
      <Waveform
        preview={{ ...preview, sampleRate: 48000, spectral: Array(128 * 64).fill(0.5) }}
        selection={{ start: 0, end: 1000 }}
        onSelection={() => {}}
        disabled={false}
        spectral
      />
    )
    for (const label of ["24 kHz", "18 kHz", "12 kHz", "6 kHz", "0 Hz"])
      expect(screen.getByText(label)).toBeInTheDocument()
  })
})
