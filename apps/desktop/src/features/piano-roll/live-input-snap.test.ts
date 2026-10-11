import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { runAction } from "@/lib/actions"
import { usePlaylistStore } from "@/features/playlist/store"
import { dispatch, undo } from "@/lib/store/project"
import * as realtime from "@/lib/store/realtime"
import { useSnapStore, type SharedSnap } from "@/lib/store/snap"
import { settle } from "@/test/harness"

import { auditionOff, auditionOn } from "./audition"
import { clearNoteLog, playedNotesFor, useNoteLogStore } from "./note-log"
import { usePianoRollStore } from "./store"
import {
  channel,
  currentPattern,
  notesOf,
  startRoll,
  undoSteps,
} from "./test-utils"

/*
 * Live input and the shared snap: what is played on a MIDI input or on the
 * computer keyboard is kept as it was played, and placed on the one snap
 * the playlist and the piano roll share when it is dumped into the pattern.
 * At 120 bpm a millisecond is 1.92 ticks.
 */
describe("live input is quantized by the shared snap", () => {
  let roll: Awaited<ReturnType<typeof startRoll>>

  beforeEach(async () => {
    vi.useFakeTimers({ toFake: ["Date", "setTimeout", "clearTimeout"] })
    vi.setSystemTime(0)
    clearNoteLog()
    roll = await startRoll()
    await dispatch({ type: "updateSettings", patch: { tempoBpm: 120 } })
    await dispatch({
      type: "clearLane",
      pattern: currentPattern().id,
      channel: channel("Kick").id,
    })
    const frame = { ...realtime.realtimeFrame() }
    vi.spyOn(realtime, "realtimeFrame").mockImplementation(() => ({
      ...frame,
      tick: 960,
    }))
  })

  afterEach(() => {
    clearNoteLog()
    roll.stop()
    useSnapStore.getState().setSnap("bar")
    vi.restoreAllMocks()
    vi.useRealTimers()
  })

  /** A key of a MIDI input held for `duration` ms, as the shell reports it. */
  function hardware(key: number, duration: number, velocity = 100) {
    const id = channel("Kick").id
    roll.backend.playMidiHardwareNote({ channel: id, key, velocity })
    vi.advanceTimersByTime(duration)
    roll.backend.playMidiHardwareNote({ channel: id, key, velocity: 0 })
  }

  /** Two notes: one on the transport position, one 590 ms (1132.8 ticks) on. */
  function phrase() {
    hardware(60, 500)
    vi.advanceTimersByTime(90)
    hardware(64, 20, 51)
  }

  it("logs MIDI input notes without sounding them twice or editing the project", () => {
    const on = vi.spyOn(roll.backend, "auditionNoteOn")
    const off = vi.spyOn(roll.backend, "auditionNoteOff")
    const send = vi.spyOn(roll.backend, "dispatch")
    const before = undoSteps()
    phrase()
    expect(playedNotesFor(channel("Kick").id)).toEqual([
      { channel: channel("Kick").id, key: 60, velocity: 100 / 127, start: 0, end: 500 },
      { channel: channel("Kick").id, key: 64, velocity: 51 / 127, start: 590, end: 610 },
    ])
    // The engine played them already; the log asks for no second audition.
    expect(on).not.toHaveBeenCalled()
    expect(off).not.toHaveBeenCalled()
    expect(send).not.toHaveBeenCalled()
    expect(undoSteps()).toBe(before)
    expect(notesOf("Kick")).toEqual([])
  })

  it.each<[SharedSnap, number, number]>([
    ["none", 960, 2093],
    ["step", 960, 2160],
    ["beat", 960, 1920],
    ["bar", 0, 3840],
  ])(
    "places MIDI input on the %s snap chosen in the playlist",
    async (snap, first, second) => {
      phrase()
      // Chosen in the other editor: the snap is one setting for both.
      usePlaylistStore.getState().setSnap(snap)
      expect(usePianoRollStore.getState().snap).toBe(snap)
      expect(useSnapStore.getState().snap).toBe(snap)
      const send = vi.spyOn(roll.backend, "dispatch")
      const before = undoSteps()

      await runAction("pianoRoll.dumpPlayedNotes")

      expect(send).toHaveBeenCalledTimes(1)
      expect(send.mock.calls[0][0]).toEqual({
        type: "addNotes",
        pattern: currentPattern().id,
        channel: channel("Kick").id,
        notes: [
          { key: 60, start: first, length: 960, velocity: 100 / 127, pan: 0 },
          { key: 64, start: second, length: 240, velocity: 51 / 127, pan: 0 },
        ],
      })
      expect(notesOf("Kick")).toMatchObject([
        { key: 60, start: first, length: 960 },
        { key: 64, start: second, length: 240 },
      ])
      expect(undoSteps()).toBe(before + 1)
      expect(useNoteLogStore.getState().notes).toEqual([])
      await undo()
      expect(notesOf("Kick")).toEqual([])
    }
  )

  it("a piano-only division does not change where live input lands", async () => {
    phrase()
    usePlaylistStore.getState().setSnap("beat")
    usePianoRollStore.getState().setSnap("step/2")
    expect(useSnapStore.getState().snap).toBe("beat")
    await runAction("pianoRoll.dumpPlayedNotes")
    expect(notesOf("Kick")).toMatchObject([
      { key: 60, start: 960 },
      { key: 64, start: 1920 },
    ])
  })

  it("keeps MIDI input and typed notes in one log, in the order they were played", async () => {
    const id = channel("Kick").id
    hardware(60, 100)
    vi.advanceTimersByTime(400)
    auditionOn(id, 67, 0.5)
    vi.advanceTimersByTime(100)
    auditionOff(id, 67)
    useSnapStore.getState().setSnap("beat")
    await runAction("pianoRoll.dumpPlayedNotes")
    // 500 ms is 960 ticks: the typed note lands a beat after the first.
    expect(notesOf("Kick")).toMatchObject([
      { key: 60, start: 960 },
      { key: 67, start: 1920 },
    ])
  })

  it("ignores a release with no press and drops MIDI input on a new project", async () => {
    const id = channel("Kick").id
    roll.backend.playMidiHardwareNote({ channel: id, key: 60, velocity: 0 })
    expect(playedNotesFor(id)).toEqual([])
    roll.backend.playMidiHardwareNote({ channel: id, key: 62, velocity: 90 })
    await roll.backend.projectNew()
    await settle()
    expect(useNoteLogStore.getState().held).toEqual([])
    roll.backend.playMidiHardwareNote({ channel: id, key: 62, velocity: 0 })
    expect(useNoteLogStore.getState().notes).toEqual([])
  })
})
