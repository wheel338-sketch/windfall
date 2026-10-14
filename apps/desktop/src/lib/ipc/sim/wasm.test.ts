import { describe, expect, it } from "vitest"

import type {
  DispatchResult,
  DocumentSnapshot,
  Project,
  ProjectPatch,
} from "@/bindings"

import { nativeMessage } from "@/lib/i18n"

import { instantiateSim, sim } from "./wasm"

/** What a failed call says, in English as the interface says a coded message. */
function said(work: () => unknown): string {
  try {
    work()
  } catch (error) {
    const text = error instanceof Error ? error.message : String(error)
    return nativeMessage(text, "en") ?? text
  }
  throw new Error("the call did not fail")
}

function open(name = "Test"): number {
  const project = sim.call<Project>("project_new", 0, name)
  return sim.call<number>("doc_new", 0, project)
}

const snapshot = (doc: number) =>
  sim.call<DocumentSnapshot>("doc_snapshot", doc, null)

describe("the WebAssembly document", () => {
  it("round trips a project, edits, undo, redo and a saved file", () => {
    const doc = open("Round trip")
    const added = sim.call<DispatchResult>("doc_dispatch", doc, {
      command: { type: "addChannel", name: "Kick" },
    })
    // The channel, then the mixer track made for it.
    expect(added.created).toEqual([2, 3])
    expect(added.patch).toMatchObject({
      revision: 1,
      dirty: true,
      history: { entries: [{ label: "Add channel" }], cursor: 1 },
    })
    expect(added.patch.channels?.[0]).toMatchObject({ id: 2, name: "Kick" })
    // Sections that did not change are left out.
    expect(added.patch.settings).toBeUndefined()

    const undone = sim.call<ProjectPatch | null>("doc_undo", doc)
    expect(undone).toMatchObject({ revision: 2, channels: [], dirty: false })
    expect(sim.call("doc_undo", doc)).toBeNull()
    const redone = sim.call<ProjectPatch | null>("doc_redo", doc)
    expect(redone?.channels?.[0].id).toBe(2)
    expect(sim.call("doc_redo", doc)).toBeNull()
    expect(sim.call<number>("doc_next_id", doc)).toBe(4)

    const text = sim.call<string>("doc_to_file_json", doc)
    const saved = sim.call<ProjectPatch>("doc_mark_saved", doc)
    expect(saved).toMatchObject({ revision: 4, dirty: false })

    const reopened = sim.call<number>("doc_from_file_json", 0, text)
    expect(reopened).not.toBe(doc)
    expect(snapshot(reopened)).toEqual({
      ...snapshot(doc),
      revision: 0,
      history: { entries: [], cursor: 0 },
    })
    sim.call("doc_free", doc)
    sim.call("doc_free", reopened)
  })

  it("carries text outside ASCII and large commands through unharmed", () => {
    const doc = open("Größe 寸法 🥁")
    expect(snapshot(doc).project.settings.name).toBe("Größe 寸法 🥁")
    sim.call("doc_dispatch", doc, { command: { type: "addChannel" } })
    const notes = Array.from({ length: 20_000 }, (_, index) => ({
      start: index * 10,
      length: 5,
      key: index % 128,
    }))
    const added = sim.call<DispatchResult>("doc_dispatch", doc, {
      command: { type: "addNotes", pattern: 1, channel: 2, notes },
    })
    expect(added.created).toHaveLength(20_000)
    expect(added.patch.patterns?.[0].lanes[0].notes).toHaveLength(20_000)
    sim.call("doc_free", doc)
  })

  it("fails with the words the Rust errors have", () => {
    const doc = open()
    const dispatch = (command: unknown) => () =>
      sim.call("doc_dispatch", doc, { command })

    // `CommandError::NotFound` and `CommandError::Invalid`.
    expect(said(dispatch({ type: "removeChannel", id: 12345 }))).toBe(
      "channel 12345 does not exist"
    )
    expect(said(dispatch({ type: "removePattern", id: 1 }))).toBe(
      "a project needs at least one pattern"
    )
    expect(said(dispatch({ type: "removeMixerTrack", id: 0 }))).toBe(
      "the master track cannot be deleted"
    )
    expect(
      said(dispatch({ type: "updatePattern", id: 1, patch: { color: 0x1000000 } }))
    ).toBe("0x1000000 is not a 0xRRGGBB color")
    // `LoadError`.
    expect(said(() => sim.call("doc_from_file_json", 0, "{}"))).toMatch(
      /^this is not a Windfall project: missing field `formatVersion`/
    )
    expect(
      said(() => sim.call("doc_from_file_json", 0, '{"formatVersion": 7}'))
    ).toBe(
      "this project was saved by a newer version of Windfall (file format 7; this version reads up to format 1)"
    )
    sim.call("doc_free", doc)
  })

  it("is left sound by a call that fails", () => {
    const doc = open()
    sim.call("doc_dispatch", doc, { command: { type: "addChannel" } })
    const before = snapshot(doc)

    const failures = [
      () =>
        sim.call("doc_dispatch", doc, { command: { type: "noSuchCommand" } }),
      () => sim.call("doc_dispatch", doc, { command: { type: "toggleStep" } }),
      () => sim.call("doc_dispatch", doc, "not an object"),
      () => sim.call("doc_dispatch", doc),
      () => sim.call("doc_jump", doc, -1),
      () => sim.call("doc_new", 0, { ...before.project, nextId: 1 }),
      () => sim.call("doc_undo", 999_999),
      () => sim.call("no_such_operation", doc),
      // A batch that fails half way takes back what it had done.
      () =>
        sim.call("doc_dispatch", doc, {
          command: {
            type: "batch",
            commands: [
              { type: "addPattern" },
              { type: "removeChannel", id: 404 },
            ],
          },
        }),
    ]
    for (const fail of failures) expect(fail).toThrow(Error)

    expect(snapshot(doc)).toEqual(before)
    const next = sim.call<DispatchResult>("doc_dispatch", doc, {
      command: { type: "addPattern" },
    })
    // No revision or id was used up by the failures.
    expect(next.patch.revision).toBe(before.revision + 1)
    expect(next.created).toEqual([before.project.nextId])
    sim.call("doc_free", doc)
    expect(said(() => sim.call("doc_free", doc))).toBe(
      `document handle ${doc} is not open`
    )
  })

  it("holds no more memory after ten thousand edits and undos", () => {
    const doc = open()
    sim.call("doc_dispatch", doc, { command: { type: "addChannel" } })
    const cycle = (step: number) => {
      sim.call("doc_dispatch", doc, {
        command: { type: "toggleStep", pattern: 1, channel: 2, step },
      })
      sim.call("doc_undo", doc)
    }
    for (let index = 0; index < 100; index += 1) cycle(index % 16)
    const settled = sim.memoryBytes()
    for (let index = 0; index < 10_000; index += 1) cycle(index % 16)
    expect(sim.memoryBytes()).toBe(settled)
    // The channel, and the one step left to redo: the history did not grow.
    const { history } = snapshot(doc)
    expect(history.entries).toHaveLength(2)
    expect(history.cursor).toBe(1)
    sim.call("doc_free", doc)
  })

  it("holds no more memory after a thousand documents come and go", () => {
    const cycle = () => {
      const doc = open()
      for (let step = 0; step < 4; step += 1) {
        sim.call("doc_dispatch", doc, { command: { type: "addChannel" } })
      }
      snapshot(doc)
      sim.call("doc_free", doc)
    }
    for (let index = 0; index < 20; index += 1) cycle()
    const settled = sim.memoryBytes()
    for (let index = 0; index < 1000; index += 1) cycle()
    expect(sim.memoryBytes()).toBe(settled)
  })

  it("says so on every call after a crash, and other modules carry on", async () => {
    const doomed = await instantiateSim()
    const project = doomed.call<Project>("project_new", 0, "Doomed")
    const doc = doomed.call<number>("doc_new", 0, project)

    const crash = () => doomed.call("sim_panic", doc, "on purpose")
    expect(crash).toThrow(
      /^The WebAssembly document crashed, and stays unusable until the page is reloaded: panicked at .*lib\.rs.*on purpose/s
    )
    // Nothing is attempted on the broken module again.
    for (const later of [
      () => doomed.call("doc_undo", doc),
      () => doomed.call("project_new", 0, "After"),
    ]) {
      expect(later).toThrow(/^The WebAssembly document crashed.*on purpose/s)
    }

    // The module the mock uses is another instance, and is untouched.
    const fine = open("Fine")
    expect(snapshot(fine).project.settings.name).toBe("Fine")
    sim.call("doc_free", fine)
  })
})
