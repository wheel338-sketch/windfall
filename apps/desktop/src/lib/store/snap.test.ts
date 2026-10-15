import { afterEach, beforeEach, describe, expect, it } from "vitest"

import { usePianoRollStore } from "@/features/piano-roll/store"
import { usePlaylistStore } from "@/features/playlist/store"
import { settle, startTestApp } from "@/test/harness"

import { useSnapStore, type SharedSnap } from "./snap"
import { useUiStore } from "./ui"

const SHARED_SNAPS: SharedSnap[] = ["none", "step", "beat", "bar"]
const playlist = () => usePlaylistStore.getState()
const piano = () => usePianoRollStore.getState()

beforeEach(() => useSnapStore.getState().setSnap("bar"))
afterEach(() => useSnapStore.getState().setSnap("bar"))

describe("shared editor snap", () => {
  it.each(SHARED_SNAPS)("a playlist choice of %s updates the piano roll", (snap) => {
    piano().setSnap("step/2")

    playlist().setSnap(snap)

    expect(playlist().snap).toBe(snap)
    expect(piano().snap).toBe(snap)
    expect(useSnapStore.getState().snap).toBe(snap)
  })

  it.each(SHARED_SNAPS)("a piano choice of %s updates the playlist", (snap) => {
    playlist().setSnap("step")
    piano().setSnap("step/2")

    piano().setSnap(snap)

    expect(playlist().snap).toBe(snap)
    expect(piano().snap).toBe(snap)
    expect(useSnapStore.getState().snap).toBe(snap)
  })

  it.each(["step/2", "beat/3"] as const)(
    "a piano-only choice of %s leaves the playlist alone",
    (snap) => {
      playlist().setSnap("beat")

      piano().setSnap(snap)

      expect(piano().snap).toBe(snap)
      expect(playlist().snap).toBe("beat")
      expect(useSnapStore.getState().snap).toBe("beat")
    }
  )

  it.each(["playlist", "piano"] as const)(
    "reselecting the same shared value in the %s clears the piano division",
    (editor) => {
      playlist().setSnap("beat")
      piano().setSnap("step/2")

      ;(editor === "playlist" ? playlist() : piano()).setSnap("beat")

      expect(playlist().snap).toBe("beat")
      expect(piano().snap).toBe("beat")
    }
  )

  it("keeps the shared choice and local division when leaving the panel", () => {
    const previousTab = useUiStore.getState().centerTab
    try {
      useUiStore.getState().showCenterTab("pianoRoll")
      piano().setSnap("none")
      piano().setSnap("step/2")

      useUiStore.getState().showCenterTab("playlist")
      expect(playlist().snap).toBe("none")
      useUiStore.getState().showCenterTab("pianoRoll")
      expect(piano().snap).toBe("step/2")
    } finally {
      useUiStore.getState().showCenterTab(previousTab)
    }
  })

  it.each(["new", "open"] as const)(
    "%s project restores bar and puts the piano roll back on step",
    async (change) => {
      const { backend, stop } = await startTestApp()
      try {
        const path = await backend.projectSave("/projects/snap.windfall")
        await backend.projectNew()
        await settle()
        playlist().setSnap("step")
        piano().setSnap("step/2")

        if (change === "open") await backend.projectOpen(path)
        else await backend.projectNew()
        await settle()

        expect(useSnapStore.getState().snap).toBe("bar")
        expect(playlist().snap).toBe("bar")
        expect(piano().snap).toBe("step")
      } finally {
        stop()
      }
    }
  )

  it("ignores legacy persisted snap choices while retaining other preferences", async () => {
    localStorage.setItem(
      "windfall.playlist",
      JSON.stringify({ state: { snap: "none", follow: false }, version: 1 })
    )
    localStorage.setItem(
      "windfall.pianoRoll",
      JSON.stringify({ state: { snap: "step/2", follow: true }, version: 1 })
    )
    const playlistFollow = playlist().follow
    const pianoFollow = piano().follow
    try {
      await usePlaylistStore.persist.rehydrate()
      await usePianoRollStore.persist.rehydrate()

      expect(playlist().snap).toBe("bar")
      expect(piano().snap).toBe("bar")
      expect(playlist().follow).toBe(false)
      expect(piano().follow).toBe(true)
    } finally {
      usePlaylistStore.setState({ follow: playlistFollow })
      piano().setFollow(pianoFollow)
    }
  })
})
