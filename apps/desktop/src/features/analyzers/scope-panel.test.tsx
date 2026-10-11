import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { registry } from "@/lib/actions"
import type { MockBackend } from "@/lib/ipc/mock"
import { useProjectStore } from "@/lib/store/project"
import { announceProjectReplaced } from "@/lib/store/replaced"
import { settle, startTestApp } from "@/test/harness"

import { registerAnalyzerActions } from "./actions"
import { resetAnalyzerFeed } from "./feed"
import { ScopePanel } from "./scope-panel"
import { closeScope, useScopeStore } from "./scope-store"
import type {
  AnalyzerEvent,
  AnalyzerFrame,
  AnalyzerLevel,
  AnalyzerSelect,
  AnalyzerSource,
  AnalyzerStatus,
} from "./types"

/*
 * The scope against a backend whose analyzer calls are watched. The events
 * handed to the slot have the shape the native service sends; what is
 * checked is which of them the views draw, and which they must not.
 */

let backend: MockBackend
let stop: () => void
let unregister: () => void
/** The handler of each subscribed slot, as the native channel would call it. */
const handlers = new Map<string, (event: AnalyzerEvent) => void>()
const stopped = vi.fn()

function status(
  slot: string,
  request: number,
  phase: AnalyzerStatus["phase"],
  source: AnalyzerSource | null = null
): AnalyzerStatus {
  return {
    slot,
    request,
    phase,
    reason: null,
    ticket: null,
    source,
    effect: null,
    sourceName: null,
    sampleRate: null,
  }
}

const level = (peak: number | null, valid = true): AnalyzerLevel => ({
  valid,
  peak,
  rms: peak === null ? null : peak / Math.SQRT2,
  peakDb: peak ? 20 * Math.log10(peak) : null,
  rmsDb: peak ? 20 * Math.log10(peak / Math.SQRT2) : null,
  silent: valid && peak === 0,
  clipped: 0,
  invalid: valid ? 0 : 3,
})

function frame(
  request: number,
  sequence: number,
  selection: string,
  levels: [AnalyzerLevel, AnalyzerLevel],
  extra: Partial<AnalyzerFrame> = {}
): AnalyzerEvent {
  return {
    type: "frame",
    slot: "scope",
    request,
    ticket: {
      service: "1",
      subscription: "1",
      stream: "1",
      selection,
      project: "0",
    },
    sequence: String(sequence),
    clock: {
      epoch: "1",
      firstFrame: String(sequence * 1024),
      endFrame: String(sequence * 1024 + 1024),
      sampleRate: 48_000,
      pdcFrames: 0,
      alignedFirstFrame: String(sequence * 1024),
      timeLabel: "00:00:00.000",
      deviceLatencyFrames: null,
    },
    gap: false,
    levels,
    stereo: { correlation: null, midRms: null, sideRms: null },
    spectrum: null,
    history: [],
    scope: null,
    vector: null,
    envelope: null,
    reductionDb: null,
    observations: [],
    counters: {
      droppedFrames: "0",
      invalidSamples: "0",
      resets: "0",
      analyzedWindows: String(sequence),
      droppedWindows: "0",
      droppedObservations: "0",
      missingSourceBlocks: "0",
      skippedEvents: "0",
    },
    playing: false,
    tick: 0,
    ...extra,
  }
}

const selections = () =>
  vi.mocked(backend.analyzerSelect).mock.calls.map(([select]) => select)
const lastSelection = (): AnalyzerSelect => {
  const all = selections()
  return all[all.length - 1]
}
const deliver = (event: AnalyzerEvent) =>
  act(() => {
    handlers.get("scope")?.(event)
  })
const side = (name: "left" | "right") => {
  const row = document.querySelector<HTMLElement>(`[data-side="${name}"]`)
  if (!row) throw new Error(`The meter has no ${name} side`)
  return row
}
const statusLine = () =>
  document.querySelector<HTMLElement>("[data-slot=analyzer-status]")

async function open() {
  render(<ScopePanel />)
  act(() => {
    void registry.get("view.signalScope")?.run()
  })
  await waitFor(() => expect(backend.analyzerSelect).toHaveBeenCalled())
  await act(() => settle())
}

beforeEach(async () => {
  handlers.clear()
  stopped.mockClear()
  resetAnalyzerFeed()
  useScopeStore.setState(useScopeStore.getInitialState(), true)
  ;({ backend, stop } = await startTestApp())
  vi.spyOn(backend, "analyzerSubscribe").mockImplementation(
    (slot, handler) => {
      handlers.set(slot, handler)
      return { status: Promise.resolve(status(slot, 0, "idle")), stop: stopped }
    }
  )
  vi.spyOn(backend, "analyzerSelect").mockImplementation((select) =>
    Promise.resolve(
      status(select.slot, select.request, "preparing", select.source)
    )
  )
  vi.spyOn(backend, "analyzerAck").mockImplementation(() => {})
  unregister = registerAnalyzerActions()
})
afterEach(() => {
  unregister()
  closeScope()
  resetAnalyzerFeed()
  stop()
  vi.restoreAllMocks()
})

describe("the signal scope", () => {
  it("opens from its action on the mix output and is live only once a frame has arrived", async () => {
    await open()
    expect(screen.getByRole("dialog", { name: "Signal scope" })).toBeVisible()
    expect(backend.analyzerSubscribe).toHaveBeenCalledTimes(1)
    const chosen = lastSelection()
    expect(chosen.slot).toBe("scope")
    expect(chosen.source).toEqual({ type: "mixOutput" })
    // The meter, the oscilloscope and the spectrum are shown, so their
    // parts are asked for and the vectorscope's is not.
    expect(chosen.want).toEqual({
      scope: true,
      spectrum: true,
      vector: false,
      envelope: false,
    })
    // Selected is not live.
    expect(statusLine()).toHaveAttribute("data-phase", "preparing")
    expect(statusLine()).toHaveTextContent("Starting…")
    expect(within(side("left")).getByText("No level")).toBeVisible()

    await deliver(frame(chosen.request, 1, "7", [level(0.5), level(0.25)]))
    expect(backend.analyzerAck).toHaveBeenLastCalledWith("scope", "1")
    await waitFor(() => expect(side("left").dataset.peakDb).toBe("-6.02"))
    expect(side("right").dataset.peakDb).toBe("-12.04")
    expect(side("left")).toHaveTextContent("−6.0 dBFS · RMS −9.0 dBFS")
    expect(
      screen.getByRole("meter", { name: "Left peak level" })
    ).toHaveAttribute("aria-valuenow", "-6.0")
    expect(statusLine()).toHaveAttribute("data-phase", "live")
    expect(statusLine()).toHaveTextContent("Live")
  })

  it("draws no frame of an earlier selection after the source is changed", async () => {
    await open()
    const first = lastSelection()
    await deliver(frame(first.request, 1, "7", [level(0.5), level(0.5)]))
    await waitFor(() => expect(side("left").dataset.peakDb).toBe("-6.02"))

    // Pick the first insert track from the source menu.
    const track = useProjectStore.getState().project.mixer.tracks[1]
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: /^Source: Mix output/ }))
    await user.click(
      await screen.findByRole("menuitemradio", { name: track.name })
    )
    await waitFor(() =>
      expect(lastSelection().source).toEqual({
        type: "trackPostFader",
        track: track.id,
      })
    )
    const second = lastSelection()
    expect(second.request).toBeGreaterThan(first.request)
    // What the mix looked like is gone at once.
    await waitFor(() => expect(side("left").dataset.peakDb).toBe(""))
    expect(
      screen.getByRole("button", { name: `Source: ${track.name}, after its fader` })
    ).toBeVisible()

    // A frame of the old selection that was still on its way: acknowledged,
    // so the service can go on, and not drawn.
    await deliver(frame(first.request, 2, "7", [level(0.9), level(0.9)]))
    expect(backend.analyzerAck).toHaveBeenLastCalledWith("scope", "2")
    await act(() => settle())
    expect(side("left").dataset.peakDb).toBe("")
    expect(statusLine()).not.toHaveAttribute("data-phase", "live")

    // The track's own audio: nothing on the left, a signal on the right.
    await deliver(frame(second.request, 3, "8", [level(0), level(0.25)]))
    await waitFor(() => expect(side("right").dataset.peakDb).toBe("-12.04"))
    expect(side("left")).toHaveTextContent("Silent")
    expect(side("left").dataset.peakDb).toBe("")
  })

  it("asks for less when a view is hidden and for more when one is shown", async () => {
    await open()
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Oscilloscope" }))
    await waitFor(() => expect(lastSelection().want.scope).toBe(false))
    expect(document.querySelector("[data-view=oscilloscope]")).toBeNull()
    expect(document.querySelector("[data-view=spectrum]")).not.toBeNull()

    await user.click(screen.getByRole("button", { name: "Vectorscope" }))
    await waitFor(() => expect(lastSelection().want.vector).toBe(true))
    expect(document.querySelector("[data-view=vectorscope]")).not.toBeNull()
    // With the spectrum hidden nothing needs it, until the spectrogram does.
    await user.click(screen.getByRole("button", { name: "Spectrum" }))
    await waitFor(() => expect(lastSelection().want.spectrum).toBe(false))
    await user.click(screen.getByRole("button", { name: "Spectrogram" }))
    await waitFor(() => expect(lastSelection().want.spectrum).toBe(true))
    expect(selections().length).toBeGreaterThanOrEqual(5)
  })

  it("shows a side with a bad sample as invalid and a stall as not live", async () => {
    await open()
    const chosen = lastSelection()
    await deliver(
      frame(chosen.request, 1, "7", [level(null, false), level(0.5)])
    )
    await waitFor(() => expect(side("right").dataset.peakDb).toBe("-6.02"))
    expect(side("left")).toHaveAttribute("data-invalid")
    expect(side("left")).toHaveTextContent("Not a valid signal")
    expect(side("left").dataset.peakDb).toBe("")

    await deliver({
      type: "status",
      ...status("scope", chosen.request, "stalled", chosen.source),
      reason: "audioStalled",
    })
    await waitFor(() =>
      expect(statusLine()).toHaveTextContent("No audio is arriving")
    )
    // What was drawn before the stall is not left standing as current.
    await waitFor(() => expect(side("right").dataset.peakDb).toBe(""))
    expect(side("right")).toHaveTextContent("No level")

    // A status for some other selection says nothing about this one.
    await deliver({
      type: "status",
      ...status("scope", chosen.request + 40, "live", chosen.source),
    })
    await act(() => settle())
    expect(statusLine()).toHaveTextContent("No audio is arriving")
  })

  it("goes back to the mix for a new project and lets go of the analyzer when closed", async () => {
    await open()
    const track = useProjectStore.getState().project.mixer.tracks[1]
    act(() =>
      useScopeStore.setState({
        source: { type: "trackPostFader", track: track.id },
      })
    )
    await waitFor(() =>
      expect(lastSelection().source.type).toBe("trackPostFader")
    )
    const before = selections().length
    act(() => announceProjectReplaced())
    await waitFor(() => expect(selections().length).toBeGreaterThan(before))
    await waitFor(() =>
      expect(lastSelection().source).toEqual({ type: "mixOutput" })
    )

    expect(stopped).not.toHaveBeenCalled()
    act(() => closeScope())
    await waitFor(() => expect(stopped).toHaveBeenCalledTimes(1))
    // Nothing arrives at a view that is gone.
    handlers.get("scope")?.(
      frame(lastSelection().request, 9, "9", [level(1), level(1)])
    )
    expect(document.querySelector("[data-slot=scope-meter]")).toBeNull()
  })
})
