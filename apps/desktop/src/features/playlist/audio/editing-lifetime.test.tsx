import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { DispatchResult } from "@/bindings"
import { getProjectGeneration } from "@/lib/store/replaced"
import { useProjectStore } from "@/lib/store/project"
import { startPlaylist, ui } from "../test-utils"
import { ClipInspector } from "./clip-inspector"

let stop: (() => void) | undefined
afterEach(() => {
  stop?.()
  vi.restoreAllMocks()
})

async function setup() {
  const app = await startPlaylist()
  stop = app.stop
  const added = await app.backend.addAudioClipFromFile(
    "/factory/Loops/Drum loop 128.wav",
    { start: 0 }
  )
  const clip = useProjectStore
    .getState()
    .project.playlist.clips.find((clip) => clip.id === added.created.at(-1))!
  ui().select([clip.id])
  const view = render(<ClipInspector />)
  return { ...app, clip, view }
}

async function pendingApply(kind: string) {
  const app = await setup()
  const { backend, clip } = app
  const replies: {
    resolve(result: DispatchResult): void
    reject(error: Error): void
  }[] = []
  function pending() {
    return new Promise<DispatchResult>((resolve, reject) => {
      replies.push({ resolve, reject })
    })
  }
  if (kind === "audio editor") {
    let token = 7
    vi.spyOn(backend, "audioEditorOpen").mockImplementation(async () => ({
      token: token++,
      clip: clip.id,
      name: "Take",
      frames: 1000,
      sampleRate: 1000,
      peaks: [-0.5, 0.5],
    }))
    vi.spyOn(backend, "audioEditorApply").mockImplementation(pending)
  } else vi.spyOn(backend, "sliceApply").mockImplementation(pending)
  const discard = vi.spyOn(
    backend,
    kind === "audio editor" ? "audioEditorDiscard" : "sliceDiscard"
  )
  async function open() {
    if (kind === "audio editor") {
      fireEvent.click(screen.getByRole("button", { name: "Audio editor" }))
      await screen.findByRole("img", { name: /Clip waveform/ })
    } else {
      fireEvent.click(screen.getByRole("button", { name: "Slice clip" }))
      fireEvent.click(screen.getByRole("button", { name: "Analyze markers" }))
      await screen.findByRole("img", { name: /Slice marker preview/ })
    }
  }
  function apply() {
    fireEvent.click(
      screen.getByRole("button", {
        name: kind === "audio editor" ? "Normalize" : "Apply slices",
      })
    )
  }
  function close() {
    fireEvent.click(
      screen.getByRole("button", {
        name: kind === "audio editor" ? "Close editor" : "Close",
      })
    )
  }
  await open()
  apply()
  return { ...app, replies, discard, open, apply, close }
}

describe("editing lifetime in the clip inspector", () => {
  it("selects all slices when their replacement event renders before the Apply reply", async () => {
    const { backend, view } = await setup()
    const generation = getProjectGeneration()
    const publish = backend.sliceApply.bind(backend)
    const discard = vi.spyOn(backend, "sliceDiscard")
    let resolve!: (result: DispatchResult) => void
    let request!: [number, number[]]
    vi.spyOn(backend, "sliceApply").mockImplementation((token, markers) => {
      request = [token, markers]
      return new Promise((done) => {
        resolve = done
      })
    })
    fireEvent.click(screen.getByRole("button", { name: "Slice clip" }))
    fireEvent.click(screen.getByRole("button", { name: "Analyze markers" }))
    await screen.findByRole("img", { name: /Slice marker preview/ })
    // Review only one cut, giving two linked-source replacement clips.
    const cuts = screen.getAllByRole("checkbox")
    for (const cut of cuts.slice(1)) fireEvent.click(cut)
    fireEvent.click(screen.getByRole("button", { name: "Apply slices" }))

    let result!: DispatchResult
    await act(async () => {
      result = await publish(...request)
    })
    expect(result.created).toHaveLength(2)
    expect(getProjectGeneration()).toBe(generation)
    view.rerender(<ClipInspector />)
    expect(screen.getByRole("dialog")).toBeInTheDocument()
    expect(discard).not.toHaveBeenCalled()
    const before = useProjectStore.getState()
    await act(async () => resolve(result))
    expect([...ui().selection]).toEqual(result.created)
    expect(screen.queryByRole("dialog")).toBeNull()
    expect(useProjectStore.getState()).toBe(before)
    expect(discard).not.toHaveBeenCalled()
  })
  it.each(["audio editor", "slicer"])(
    "abandons %s after selection changes, even if the original selection returns before React renders",
    async (kind) => {
      const { backend, clip, replies, discard } = await pendingApply(kind)
      await act(async () => {
        ui().clearSelection()
        ui().select([clip.id])
      })
      expect(screen.queryByRole("dialog")).toBeNull()
      expect(discard).toHaveBeenCalledOnce()

      // A late successful reply alone must neither publish a stale patch
      // nor retake selection after the user has left this operation.
      let result!: DispatchResult
      await act(async () => {
        result = await backend.dispatch({ type: "addClips", clips: [clip] })
      })
      const before = useProjectStore.getState()
      await act(async () => replies[0].resolve(result))
      expect([...ui().selection]).toEqual([clip.id])
      expect(useProjectStore.getState()).toBe(before)
    }
  )
  it("selects the edited clip when its replacement event renders before the Apply reply", async () => {
    const { backend, clip, view } = await setup()
    const generation = getProjectGeneration()
    vi.spyOn(backend, "audioEditorOpen").mockResolvedValue({
      token: 7,
      clip: clip.id,
      name: "Take",
      frames: 1000,
      sampleRate: 1000,
      peaks: [-0.5, 0.5],
    })
    const discard = vi.spyOn(backend, "audioEditorDiscard")
    let resolve!: (result: DispatchResult) => void
    vi.spyOn(backend, "audioEditorApply").mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done
        })
    )
    fireEvent.click(screen.getByRole("button", { name: "Audio editor" }))
    await screen.findByRole("img", { name: /Clip waveform/ })
    fireEvent.click(screen.getByRole("button", { name: "Normalize" }))

    // Real WASM document publication reaches the connected stores first.
    // Keep the native reply pending until React has rendered that event.
    let result!: DispatchResult
    await act(async () => {
      result = await backend.dispatch({
        type: "batch",
        label: "Normalize audio",
        commands: [
          { type: "removeClips", clips: [clip.id] },
          { type: "addClips", clips: [clip] },
        ],
      })
    })
    expect(getProjectGeneration()).toBe(generation)
    expect(
      useProjectStore
        .getState()
        .project.playlist.clips.some((current) => current.id === clip.id)
    ).toBe(false)
    view.rerender(<ClipInspector />)
    expect(screen.getByRole("dialog")).toBeInTheDocument()
    expect(discard).not.toHaveBeenCalled()
    const before = useProjectStore.getState()
    await act(async () => resolve(result))
    expect([...ui().selection]).toEqual(result.created)
    // Destructive audio edits keep the editor open on the new owned source.
    expect(screen.getByRole("dialog")).toBeVisible()
    expect(backend.audioEditorOpen).toHaveBeenLastCalledWith(result.created.at(-1))
    expect(useProjectStore.getState()).toBe(before)
    expect(discard).toHaveBeenCalledWith(7)
  })
  it.each(["audio editor", "slicer"])(
    "releases %s ownership when the inspector is hidden",
    async (kind) => {
      const { backend, clip, replies, discard, open, close } =
        await pendingApply(kind)
      let result!: DispatchResult
      await act(async () => {
        result = await backend.dispatch({ type: "addClips", clips: [clip] })
        ui().toggleInspector()
      })
      expect(screen.queryByRole("dialog")).toBeNull()
      expect(discard).toHaveBeenCalledOnce()
      const before = useProjectStore.getState()
      await act(async () => replies[0].resolve(result))
      expect([...ui().selection]).toEqual([clip.id])
      expect(useProjectStore.getState()).toBe(before)
      await act(async () => ui().toggleInspector())
      await open()
      // Closing the reopened editor proves its parent isn't left busy.
      close()
      expect(screen.queryByRole("dialog")).toBeNull()
      expect(discard).toHaveBeenCalledTimes(2)
    }
  )
  it.each(["audio editor", "slicer"])(
    "ignores an old %s reply in a reopened project with reused clip IDs and revisions",
    async (kind) => {
      const app = await pendingApply(kind)
      const { backend, clip, replies, discard, open, close } = app
      const generation = getProjectGeneration()
      let result!: DispatchResult
      await act(async () => {
        const saved = await backend.projectSave(
          "/saved/Editing lifetime.windfall"
        )
        result = await backend.dispatch({
          type: "batch",
          commands: [
            { type: "removeClips", clips: [clip.id] },
            { type: "addClips", clips: [clip] },
          ],
        })
        await backend.projectOpen(saved)
        expect(useProjectStore.getState().project.playlist.clips[0].id).toBe(
          clip.id
        )
        ui().select([clip.id])
      })
      expect(getProjectGeneration()).not.toBe(generation)
      expect(useProjectStore.getState().revision).toBe(0)
      // A reply with a revision that is valid in the newly opened document
      // still belongs to the old generation, even when its source ID exists.
      result = { ...result, patch: { ...result.patch, revision: 1 } }
      expect(discard).toHaveBeenCalledOnce()
      await open()
      const before = useProjectStore.getState()
      await act(async () => replies[0].resolve(result))
      expect(useProjectStore.getState()).toBe(before)
      expect([...ui().selection]).toEqual([clip.id])
      expect(screen.getByRole("dialog")).toBeInTheDocument()
      close()
      expect(discard).toHaveBeenCalledTimes(2)
    }
  )
  it.each(["audio editor", "slicer"])(
    "keeps a newer %s Apply busy when an abandoned operation replies",
    async (kind) => {
      const { backend, clip, replies, discard, open, apply, close } =
        await pendingApply(kind)
      let result!: DispatchResult
      await act(async () => {
        result = await backend.dispatch({ type: "addClips", clips: [clip] })
        ui().clearSelection()
        ui().select([clip.id])
      })
      expect(discard).toHaveBeenCalledOnce()
      await open()
      apply()
      expect(replies).toHaveLength(2)
      await act(async () => replies[0].resolve(result))
      expect([...ui().selection]).toEqual([clip.id])
      expect(screen.getByRole("dialog")).toBeInTheDocument()
      expect(
        screen.getByRole("button", {
          name: kind === "audio editor" ? "Normalize" : "Apply slices",
        })
      ).toBeDisabled()
      if (kind === "audio editor")
        expect(
          screen.getByRole("button", { name: "Close editor" })
        ).toBeDisabled()
      await act(async () =>
        replies[1].reject(new Error("New operation failed"))
      )
      expect(screen.getByRole("alert")).toHaveTextContent(
        "New operation failed"
      )
      close()
      expect(screen.queryByRole("dialog")).toBeNull()
      expect(discard).toHaveBeenCalledTimes(2)
    }
  )
  it("releases a slice review on Close and ignores its pending Apply reply", async () => {
    const { backend, clip, replies, discard, close } =
      await pendingApply("slicer")
    let result!: DispatchResult
    await act(async () => {
      result = await backend.dispatch({
        type: "batch",
        commands: [
          { type: "removeClips", clips: [clip.id] },
          { type: "addClips", clips: [clip] },
        ],
      })
    })
    expect(screen.getByRole("dialog")).toBeInTheDocument()
    close()
    expect(discard).toHaveBeenCalledOnce()
    const before = useProjectStore.getState()
    await act(async () => replies[0].resolve(result))
    expect(screen.queryByRole("dialog")).toBeNull()
    expect([...ui().selection]).toEqual([clip.id])
    expect(useProjectStore.getState()).toBe(before)
  })
})
