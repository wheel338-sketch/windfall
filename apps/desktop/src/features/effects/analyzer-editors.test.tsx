import { act, render, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { EffectId, EffectKind, TrackId } from "@/bindings"
import { ValueContextMenus } from "@/components/value-context-menu"
import { effectSlot } from "@/features/analyzers/effect-slot"
import { resetAnalyzerFeed } from "@/features/analyzers/feed"
import type {
  AnalyzerEvent,
  AnalyzerFrame,
  AnalyzerObservation,
  AnalyzerSelect,
  AnalyzerStatus,
} from "@/features/analyzers/types"
import { effectDescriptor, paramIndex } from "@/features/params"
import type { MockBackend } from "@/lib/ipc/mock"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { settle, startTestApp } from "@/test/harness"

import { EffectEditor } from "./effect-editor"

/*
 * The live parts of the effect editors: the limiter's history, the tuner's
 * readout and the equaliser's spectrum. Each is handed events in the shape
 * the native analyzer sends and must show exactly those, for its own
 * effect's slot, and nothing once they stop being current.
 */

let backend: MockBackend
let stop: () => void
let track: TrackId
const handlers = new Map<string, (event: AnalyzerEvent) => void>()

function status(
  select: Pick<AnalyzerSelect, "slot" | "request" | "source">,
  phase: AnalyzerStatus["phase"],
  reason: AnalyzerStatus["reason"] = null
): AnalyzerStatus {
  return {
    slot: select.slot,
    request: select.request,
    phase,
    reason,
    ticket: null,
    source: select.source,
    effect: null,
    sourceName: null,
    sampleRate: null,
  }
}

function frame(
  select: AnalyzerSelect,
  sequence: number,
  extra: Partial<AnalyzerFrame>
): AnalyzerEvent {
  const quiet = {
    valid: true,
    peak: 0,
    rms: 0,
    peakDb: null,
    rmsDb: null,
    silent: true,
    clipped: 0,
    invalid: 0,
  }
  return {
    type: "frame",
    slot: select.slot,
    request: select.request,
    ticket: {
      service: "1",
      subscription: "1",
      stream: "1",
      selection: "5",
      project: "0",
    },
    sequence: String(sequence),
    clock: {
      epoch: "1",
      firstFrame: "0",
      endFrame: "1024",
      sampleRate: 48_000,
      pdcFrames: 0,
      alignedFirstFrame: "0",
      timeLabel: "00:00:00.000",
      deviceLatencyFrames: null,
    },
    gap: false,
    levels: [quiet, quiet],
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
      analyzedWindows: "1",
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

function Editor({ effect }: { effect: EffectId }) {
  const slot = useProjectStore((state) =>
    state.project.mixer.tracks
      .find((item) => item.id === track)
      ?.effects.find((item) => item.id === effect)
  )
  return slot ? <EffectEditor trackId={track} slot={slot} /> : null
}

async function open(kind: EffectKind): Promise<EffectId> {
  const result = await dispatch({ type: "addEffect", track, kind })
  if (!result) throw new Error(`Could not add a ${kind}`)
  const effect = result.created[0]
  render(
    <ValueContextMenus>
      <Editor effect={effect} />
    </ValueContextMenus>
  )
  await act(() => settle())
  return effect
}

const selections = () =>
  vi.mocked(backend.analyzerSelect).mock.calls.map(([select]) => select)

/** The selection an effect's editor made, once it has made it. */
async function selected(effect: EffectId): Promise<AnalyzerSelect> {
  const slot = effectSlot(effect)
  await waitFor(() =>
    expect(selections().some((select) => select.slot === slot)).toBe(true)
  )
  await act(() => settle())
  const mine = selections().filter((select) => select.slot === slot)
  return mine[mine.length - 1]
}

const deliver = (select: AnalyzerSelect, event: AnalyzerEvent) =>
  act(() => {
    handlers.get(select.slot)?.(event)
  })

const one = (selector: string) => {
  const found = document.querySelector<HTMLElement>(selector)
  if (!found) throw new Error(`Nothing matches ${selector}`)
  return found
}

function watchAnalyzer() {
  vi.spyOn(backend, "analyzerSubscribe").mockImplementation(
    (slot, handler) => {
      handlers.set(slot, handler)
      return {
        status: Promise.resolve({
          slot,
          request: 0,
          phase: "idle",
          reason: null,
          ticket: null,
          source: null,
          effect: null,
          sourceName: null,
          sampleRate: null,
        }),
        stop: () => handlers.delete(slot),
      }
    }
  )
  vi.spyOn(backend, "analyzerSelect").mockImplementation((select) =>
    Promise.resolve(status(select, "awaitingAudio"))
  )
  vi.spyOn(backend, "analyzerAck").mockImplementation(() => {})
}

beforeEach(async () => {
  handlers.clear()
  resetAnalyzerFeed()
  ;({ backend, stop } = await startTestApp())
  track = useProjectStore.getState().project.mixer.tracks[1].id
})
afterEach(() => {
  resetAnalyzerFeed()
  stop()
  vi.restoreAllMocks()
})

const dynamics = (
  firstFrame: number,
  reductionDb: number
): AnalyzerObservation => ({
  type: "dynamics",
  firstFrame: String(firstFrame),
  frames: 256,
  inputPeak: 1.5,
  outputPeak: 0.5,
  reductionDb,
  gateDb: 0,
})

describe("the limiter's level history", () => {
  it("says in a browser that it needs the desktop app, and draws nothing", async () => {
    await open("limiter")
    const line = one("[data-slot=limiter-editor] [data-slot=analyzer-status]")
    await waitFor(() => expect(line).toHaveAttribute("data-phase", "unavailable"))
    expect(line).toHaveTextContent("Needs the desktop app's audio engine")
    expect(one("[data-slot=limiter-history]").dataset.blocks).toBe("0")
  })

  it("is bound to its own limiter on its own track and counts the blocks it is sent", async () => {
    watchAnalyzer()
    const effect = await open("limiter")
    const select = await selected(effect)
    expect(select.source).toEqual({ type: "trackPostFader", track })
    expect(select.effect).toBe(effect)
    // The history needs the limiter's readings, not a spectrum.
    expect(select.want).toEqual({
      scope: false,
      spectrum: false,
      vector: false,
      envelope: false,
    })
    const history = one("[data-slot=limiter-history]")
    expect(history.dataset.blocks).toBe("0")

    await deliver(
      select,
      frame(select, 1, {
        observations: [dynamics(0, 0), dynamics(256, 4), dynamics(512, 9.5)],
      })
    )
    expect(backend.analyzerAck).toHaveBeenLastCalledWith(select.slot, "1")
    expect(history.dataset.blocks).toBe("3")
    expect(history.dataset.reductionDb).toBe("9.50")

    // After a break the picture starts over: nothing is joined across it.
    await deliver(
      select,
      frame(select, 2, { gap: true, observations: [dynamics(9_000, 2)] })
    )
    expect(history.dataset.blocks).toBe("1")

    // A tuner's reading on a limiter's slot is not a block of dynamics.
    await deliver(
      select,
      frame(select, 3, {
        observations: [
          {
            type: "tuner",
            firstFrame: "9256",
            frames: 256,
            frequencyHz: 440,
            midiNote: 69,
            cents: 0,
            confidence: 1,
          },
        ],
      })
    )
    expect(history.dataset.blocks).toBe("1")

    await deliver(select, {
      type: "status",
      ...status(select, "refused", "effectMissing"),
    })
    expect(history.dataset.blocks).toBe("0")
    expect(history.dataset.reductionDb).toBeUndefined()
  })

  it("marks the compression and the gate as at work only when the settings say so", async () => {
    const effect = await open("limiter")
    const descriptor = effectDescriptor("limiter")
    const compression = one("[data-slot=limiter-compression]")
    const gate = one("[data-slot=limiter-gate]")
    expect(compression).not.toHaveAttribute("data-active")
    expect(gate).not.toHaveAttribute("data-active")
    for (const id of [
      "mode",
      "thresholdDb",
      "ratio",
      "kneeDb",
      "attackMs",
      "gateThresholdDb",
      "gateReleaseMs",
    ]) {
      expect(document.querySelector(`[data-param="${id}"]`), id).not.toBeNull()
    }

    await dispatch({
      type: "setEffectParam",
      track,
      effect,
      param: paramIndex(descriptor, "mode"),
      value: 1,
    })
    await act(() => settle())
    expect(compression).toHaveAttribute("data-active")

    await dispatch({
      type: "setEffectParam",
      track,
      effect,
      param: paramIndex(descriptor, "gateThresholdDb"),
      value: -40,
    })
    await act(() => settle())
    expect(gate).toHaveAttribute("data-active")
  })
})

describe("the tuner's readout", () => {
  it("shows the note and the cents of the newest reading, and no note without a pitch", async () => {
    watchAnalyzer()
    const effect = await open("tuner")
    expect(one("[data-slot=tuner-editor]")).toBeVisible()
    expect(document.querySelector('[data-param="referenceHz"]')).not.toBeNull()
    const select = await selected(effect)
    expect(select.effect).toBe(effect)
    const note = one("[data-slot=tuner-note]")
    const meter = one("[data-slot=tuner-meter]")
    expect(note).toHaveTextContent("–")
    expect(meter).toHaveAttribute("aria-valuetext", "No steady pitch")

    const reading = (
      cents: number | null,
      midiNote: number | null
    ): AnalyzerObservation => ({
      type: "tuner",
      firstFrame: "0",
      frames: 256,
      frequencyHz: cents === null ? null : 440,
      midiNote,
      cents,
      confidence: 0.97,
    })
    await deliver(
      select,
      frame(select, 1, { observations: [reading(-20, 69), reading(31.8, 69)] })
    )
    await waitFor(() => expect(note).toHaveTextContent("A4"))
    expect(one("[data-slot=tuner-cents]")).toHaveTextContent("+31.8 ¢")
    expect(one("[data-slot=tuner-frequency]")).toHaveTextContent("440.0 Hz")
    expect(meter).toHaveAttribute("aria-valuetext", "A4, +31.8 ¢, sharp")
    expect(meter).toHaveAttribute("aria-valuenow", "31.8")
    expect(one("[data-slot=tuner-editor]")).not.toHaveAttribute("data-in-tune")

    await deliver(select, frame(select, 2, { observations: [reading(2, 69)] }))
    await waitFor(() =>
      expect(one("[data-slot=tuner-editor]")).toHaveAttribute("data-in-tune")
    )

    await deliver(
      select,
      frame(select, 3, { observations: [reading(null, null)] })
    )
    await waitFor(() => expect(note).toHaveTextContent("–"))
    expect(meter).toHaveAttribute("aria-valuetext", "No steady pitch")
    expect(meter).not.toHaveAttribute("aria-valuenow")
  })
})

describe("the equaliser's live spectrum", () => {
  it("asks for the spectrum of its own track and shows where the sound is", async () => {
    watchAnalyzer()
    const effect = await open("eq")
    const select = await selected(effect)
    expect(select.source).toEqual({ type: "trackPostFader", track })
    // The spectrum is the track's audio, not a reading of the equaliser.
    expect(select.effect).toBeUndefined()
    expect(select.want.spectrum).toBe(true)
    const spectrum = one("[data-slot=eq-live-spectrum]")
    expect(spectrum.dataset.peakHz).toBeUndefined()
    const line = one("[data-slot=eq-editor] [data-slot=analyzer-status]")
    expect(line).toHaveTextContent("Waiting for audio")

    // A tone in bin 21 of 513 at 48 kHz: 984 Hz, at a quarter power.
    const left = new Array<number>(513).fill(0)
    left[21] = 0.25
    await deliver(
      select,
      frame(select, 1, {
        levels: [{ valid: true, silent: false, peak: 0.5, rms: Math.SQRT1_2 * 0.5, peakDb: 20 * Math.log10(0.5), rmsDb: 20 * Math.log10(Math.SQRT1_2 * 0.5), clipped: 0, invalid: 0 }, { valid: true, silent: false, peak: 0.5, rms: Math.SQRT1_2 * 0.5, peakDb: 20 * Math.log10(0.5), rmsDb: 20 * Math.log10(Math.SQRT1_2 * 0.5), clipped: 0, invalid: 0 }],
        spectrum: { binHz: 48_000 / 1024, enbwBins: 1.5, left, right: [...left], bands: [] },
      })
    )
    const hz = Number(spectrum.dataset.peakHz)
    expect(hz).toBeGreaterThan(984 / 1.05)
    expect(hz).toBeLessThan(984 * 1.05)
    expect(spectrum.dataset.peakDb).toBe("-6.0")
    await waitFor(() => expect(line).toHaveTextContent("Live"))

    // No sound, or no longer current: nothing is left to read.
    await deliver(select, {
      type: "status",
      ...status(select, "stalled", "audioStalled"),
    })
    expect(spectrum.dataset.peakHz).toBeUndefined()
    await waitFor(() => expect(line).toHaveTextContent("No audio is arriving"))
  })
})
