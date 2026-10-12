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
})
