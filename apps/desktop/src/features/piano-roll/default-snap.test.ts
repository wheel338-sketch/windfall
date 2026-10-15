import { afterEach, describe, expect, it } from "vitest"

import { announceProjectReplaced } from "@/lib/store/replaced"
import { useSnapStore } from "@/lib/store/snap"

import { DEFAULT_SNAP } from "./snap"
import { liveInputSnap, usePianoRollStore } from "./store"

const piano = () => usePianoRollStore.getState()

afterEach(() => useSnapStore.getState().setSnap("bar"))

describe("the piano roll's starting snap", () => {
  it("is Step after a new project, while the shared snap stays on Bar", () => {
    piano().setSnap("beat/3")
    announceProjectReplaced()
    expect(DEFAULT_SNAP).toBe("step")
    expect(useSnapStore.getState().snap).toBe("bar")
    expect(piano().snap).toBe("step")
    // Typed and played notes land on what the piano roll shows.
    expect(liveInputSnap()).toBe("step")
  })

  it("follows a shared choice made in either editor", () => {
    announceProjectReplaced()
    useSnapStore.getState().setSnap("bar")
    expect(piano().snap).toBe("bar")
    piano().setSnap("beat")
    expect(useSnapStore.getState().snap).toBe("beat")
    expect(liveInputSnap()).toBe("beat")
  })

  it("leaves live input on the shared snap under a piano-only division", () => {
    useSnapStore.getState().setSnap("beat")
    piano().setSnap("step/2")
    expect(liveInputSnap()).toBe("beat")
  })
})
