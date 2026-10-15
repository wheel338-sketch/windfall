import { act, cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import type { RealtimeFrame } from "@/bindings"
import {
  emptyDropoutLog,
  noteDropouts,
  PerformanceReadout,
  shownDropouts,
} from "./performance-readout"

const feed = vi.hoisted(() => ({
  draw: (frame: Readonly<RealtimeFrame>) => {
    void frame
  },
}))
vi.mock("@/lib/store/realtime", async (original) => ({
  ...(await original<typeof import("@/lib/store/realtime")>()),
  useRealtime: (draw: typeof feed.draw) => {
    feed.draw = draw
  },
}))
vi.mock("./memory-readout", () => ({ MemoryReadout: () => null }))

let clock = 0
beforeEach(() => {
  clock = 0
  // Every frame is drawn: each one comes a second after the one before.
  vi.spyOn(performance, "now").mockImplementation(() => (clock += 1000))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

function frame(xruns: number, playing = false): RealtimeFrame {
  return {
    playing,
    tick: 0,
    meters: [],
    cpu: 0.04,
    xruns,
    voices: 0,
    gainReductions: [],
    automated: [],
    audioClips: 0,
    droppedClips: 0,
  }
}

it("notes when each dropout came and what the transport was doing", () => {
  let log = emptyDropoutLog()
  log = noteDropouts(log, frame(0), 10)
  expect(log.lastAt).toBeNull()
  log = noteDropouts(log, frame(2, true), 20)
  expect(log).toMatchObject({ seen: 2, lastAt: 20, lastPlaying: true, lastCpu: 0.04 })
  expect(noteDropouts(log, frame(2), 30)).toBe(log)
  expect(shownDropouts({ ...log, baseline: 1 })).toBe(1)
})

it("starts again when a reopened output starts its count again", () => {
  const log = noteDropouts({ ...emptyDropoutLog(), seen: 5, baseline: 3, lastAt: 1 }, frame(1), 2)
  expect(log).toMatchObject({ seen: 1, baseline: 0, lastAt: null })
})

it("opens the dropouts' context from the count and resets it", async () => {
  const user = userEvent.setup()
  render(<PerformanceReadout />)
  expect(screen.queryByRole("button", { name: /dropout/ })).toBeNull()
  act(() => feed.draw(frame(3)))
  const count = screen.getByRole("button", { name: "3 dropouts" })
  await user.click(count)
  expect(await screen.findByText("Audio dropouts")).toBeVisible()
  expect(screen.getByText(/while stopped$/)).toBeVisible()
  expect(screen.getByText(/raise the buffer size/)).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Reset count" }))
  expect(screen.queryByRole("button", { name: /dropout/ })).toBeNull()
  act(() => feed.draw(frame(4, true)))
  expect(screen.getByRole("button", { name: "1 dropout" })).toBeVisible()
})
