import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { DocumentSnapshot, ProjectPatch } from "@/bindings"
import type { Backend } from "@/lib/ipc"
import { settle, startTestApp, toastWords } from "@/test/harness"

import {
  dispatch,
  historyJump,
  receivePatch,
  redo,
  setProjectPath,
  undo,
  useProjectStore,
} from "./project"
import { selectedPatternId } from "./selectors"
import { setTransportPattern, useTransportStore } from "./transport"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

const { toast } = await import("sonner")

let backend: Backend
let stop: () => void

beforeEach(async () => {
  vi.mocked(toast.error).mockClear()
  ;({ backend, stop } = await startTestApp())
})

afterEach(() => stop())

const state = () => useProjectStore.getState()

/** A patch that carries nothing but the history of a snapshot. */
function patchAt(snapshot: DocumentSnapshot): ProjectPatch {
  return {
    revision: snapshot.revision,
    history: snapshot.history,
    dirty: snapshot.dirty,
  }
}

describe("project store", () => {
  it("loads the snapshot when the app connects", () => {
    expect(state()).toMatchObject({ ready: true, revision: 0, dirty: false })
    expect(state().project.channels).toHaveLength(4)
    expect(useTransportStore.getState().pattern).toBe(
      state().project.patterns[0].id
    )
  })

  it("changes only through the backend and follows its patches", async () => {
    const result = await dispatch({ type: "addChannel", name: "Bass" })
    expect(result?.created).toHaveLength(2)
    expect(state().revision).toBe(1)
    expect(state().dirty).toBe(true)
    expect(state().project.channels.at(-1)?.name).toBe("Bass")
    expect(state().history).toEqual({
      entries: [{ label: "Add channel" }],
      cursor: 1,
    })
  })

  it("applies each patch once though it arrives by event and as the result", async () => {
    await dispatch({ type: "addPattern" })
    await dispatch({ type: "addPattern" })
    expect(state().revision).toBe(2)
    expect(state().project.patterns).toHaveLength(3)
  })

  it("follows edits made somewhere else, such as another window", async () => {
    await backend.dispatch({ type: "updateSettings", patch: { tempoBpm: 90 } })
    expect(state().project.settings.tempoBpm).toBe(90)
  })

  it("shows a failed command and returns null", async () => {
    const result = await dispatch({ type: "removeChannel", id: 4242 })
    expect(result).toBeNull()
    // The document says "channel 4242 does not exist". It is shown as a sentence.
    expect(vi.mocked(toast.error).mock.calls.map(([title]) => toastWords(title))).toContainEqual("Channel 4242 does not exist")
    expect(state().revision).toBe(0)
  })

  it("undoes, redoes and jumps through the history", async () => {
    await dispatch({ type: "updateSettings", patch: { tempoBpm: 100 } })
    await dispatch({ type: "updateSettings", patch: { tempoBpm: 110 } })

    await undo()
    expect(state().project.settings.tempoBpm).toBe(100)
    await redo()
    expect(state().project.settings.tempoBpm).toBe(110)
    await historyJump(0)
    expect(state().project.settings.tempoBpm).toBe(128)
    expect(state().history.cursor).toBe(0)
    expect(state().dirty).toBe(false)
  })

  it("makes one undo step of a gesture", async () => {
    for (const tempoBpm of [100, 101, 102]) {
      await dispatch({ type: "updateSettings", patch: { tempoBpm } }, 55)
    }
    expect(state().history.entries).toHaveLength(1)
    await undo()
    expect(state().project.settings.tempoBpm).toBe(128)
  })

  it("fetches the snapshot again when a patch was missed", async () => {
    const snapshot = vi.spyOn(backend, "documentSnapshot")
    const missed: ProjectPatch[] = []
    const off = backend.onProjectPatch((patch) => missed.push(patch))

    // Two edits happen, but this window only ever hears about the second.
    useProjectStore.setState({ revision: 0 })
    const before = state().project
    await backend.dispatch({ type: "addPattern" })
    await backend.dispatch({ type: "addPattern" })
    useProjectStore.setState({ revision: 0, project: before })
    off()

    receivePatch(missed[1])
    expect(state().revision).toBe(0)
    await settle()

    expect(snapshot).toHaveBeenCalledTimes(1)
    expect(state().revision).toBe(2)
    expect(state().project.patterns).toHaveLength(3)
  })

  it("fetches again when a patch newer than the snapshot came in meanwhile", async () => {
    await dispatch({ type: "addPattern" })
    const first = await backend.documentSnapshot()
    await backend.dispatch({ type: "addPattern" })
    await backend.dispatch({ type: "addPattern" })
    // This window missed both edits. The snapshot it asks for is slow: it
    // was taken before them, and the patch of the third edit overtakes it.
    useProjectStore.setState({ revision: 1, project: first.project })
    const newest = await backend.documentSnapshot()
    const snapshot = vi
      .spyOn(backend, "documentSnapshot")
      .mockResolvedValueOnce({ ...first, revision: 2 })

    receivePatch({ ...patchAt(newest), revision: 3 })
    receivePatch({ ...patchAt(newest), revision: 4 })
    await settle()

    // The first answer was older than a patch already seen, so it asked again.
    expect(snapshot).toHaveBeenCalledTimes(2)
    expect(state().revision).toBe(newest.revision)
    expect(state().project.patterns).toHaveLength(4)
  })

  it("records where a save went and leaves the unsaved mark to the backend", async () => {
    await dispatch({ type: "addPattern" })
    setProjectPath("/projects/song.windfall")
    expect(state()).toMatchObject({
      path: "/projects/song.windfall",
      dirty: true,
    })
  })

  it("keeps a channel's reference when another channel changes", async () => {
    const [first, second] = state().project.channels
    await dispatch({
      type: "updateChannel",
      id: first.id,
      patch: { pan: -0.5 },
    })
    expect(state().project.channels[0]).not.toBe(first)
    expect(state().project.channels[1]).toBe(second)
  })
})

describe("selected pattern", () => {
  it("is the transport's pattern", async () => {
    const result = await dispatch({ type: "addPattern" })
    const added = result?.created[0] ?? -1
    await setTransportPattern(added)
    expect(useTransportStore.getState().pattern).toBe(added)
    expect(selectedPatternId(state().project, added)).toBe(added)
  })

  it("falls back to the first pattern when the selected one is deleted", async () => {
    const result = await dispatch({ type: "addPattern" })
    const added = result?.created[0] ?? -1
    await setTransportPattern(added)
    await dispatch({ type: "removePattern", id: added })

    const first = state().project.patterns[0].id
    expect(useTransportStore.getState().pattern).toBe(first)
    expect(selectedPatternId(state().project, added)).toBe(first)
  })
})
