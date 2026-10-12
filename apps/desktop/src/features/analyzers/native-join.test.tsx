/// <reference types="node" />
import { readFileSync } from "node:fs"
import { resolve } from "node:path"
import { act, render, waitFor } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"

import type { DocumentSnapshot, Project, RealtimeFrame } from "@/bindings"
import { EqLiveSpectrum } from "@/features/effects/eq/live-spectrum"
import { LimiterHistory } from "@/features/effects/limiter/history"
import { TunerReadout } from "@/features/effects/tuner/tuner-editor"
import { StripWaveform } from "@/features/mixer/waveform-meter"
import { LargeClock } from "@/features/stage/large-clock"
import { LargeMasterMeter } from "@/features/stage/large-master-meter"
import { loadSnapshot, useProjectStore } from "@/lib/store/project"
import { announceProjectReplaced } from "@/lib/store/replaced"
import { useTransportStore } from "@/lib/store/transport"
import { formatMusicalPosition } from "@/lib/timeline"
import { formatClock, ticksToSeconds } from "@/lib/time"
import { settle, startTestApp } from "@/test/harness"

import { resetAnalyzerFeed, useAnalyzerSelection } from "./feed"
import { AnalyzerStatusLine } from "./status"
import type { AnalyzerEvent, AnalyzerFrame, AnalyzerSelect, AnalyzerStatus } from "./types"
import { Oscilloscope, PeakMeter, Spectrogram, SpectrumCurve, Vectorscope } from "./views"

type Step = {
  name: string
  request: number
  select?: AnalyzerSelect
  answer?: AnalyzerStatus
  analyzer: string[]
  realtime: string[]
  projectLoaded?: string
}
type Fixture = { schema: number; project: Project; tracks: [number, number]; steps: Step[] }

function JoinedViews({ select, tracks }: { select: AnalyzerSelect; tracks: number[] }) {
  useAnalyzerSelection("scope", {
    source: select.source, effect: select.effect ?? undefined, want: select.want,
  })
  return <>
    <LargeClock /><LargeMasterMeter />
    {tracks.map((id) => <StripWaveform key={id} id={id} vertical={false} wide active />)}
    <PeakMeter slot="scope" floorDb={-90} />
    <Oscilloscope slot="scope" span={1024} triggered={false} />
    <SpectrumCurve slot="scope" floorDb={-90} />
    <Spectrogram slot="scope" floorDb={-90} />
    <Vectorscope slot="scope" />
    <EqLiveSpectrum slot="scope" /><LimiterHistory slot="scope" />
    <TunerReadout slot="scope" /><AnalyzerStatusLine slot="scope" />
  </>
}

let stop: (() => void) | undefined
afterEach(() => { resetAnalyzerFeed(); stop?.(); vi.restoreAllMocks(); vi.unstubAllGlobals() })

const element = (selector: string) => {
  const found = document.querySelector<HTMLElement>(selector)
  if (!found) throw new Error(`Missing real display ${selector}`)
  return found
}
const datasets = (slot: string) => element(`[data-slot=${slot}]`).dataset

it("replays real native session channel bytes through the mounted displays and every source lifecycle", async () => {
  // A missing capture is a failure, never a synthetic substitute or skipped test.
  const file = process.env.WINDFALL_ANALYZER_CAPTURE_DIR
    ? resolve(import.meta.dirname, "../../../../..", process.env.WINDFALL_ANALYZER_CAPTURE_DIR, "native-join.json")
    : resolve(import.meta.dirname, "fixtures/native-join.json")
  const fixture = JSON.parse(readFileSync(file, "utf8")) as Fixture
  expect(fixture.schema).toBe(1)
  const app = await startTestApp()
  stop = app.stop
  const { backend } = app
  useProjectStore.setState({ project: fixture.project, ready: true })
  useTransportStore.setState({ mode: "song" })
  let analyzer: ((event: AnalyzerEvent) => void) | undefined
  let realtime: ((frame: RealtimeFrame) => void) | undefined
  let answer: AnalyzerStatus | undefined
  vi.spyOn(backend, "analyzerSubscribe").mockImplementation((slot, handler) => {
    analyzer = handler
    return { status: Promise.resolve({ slot, request: 0, phase: "idle", reason: null, ticket: null, source: null, effect: null, sourceName: null, sampleRate: null }), stop: () => { analyzer = undefined } }
  })
  vi.spyOn(backend, "analyzerSelect").mockImplementation(async (select) => {
    expect(select.request).toBe(answer?.request)
    expect(select.source).toEqual(answer?.source)
    if (!answer) throw new Error("No recorded native selection reply")
    return answer
  })
  vi.spyOn(backend, "analyzerAck").mockImplementation(() => {})
  vi.spyOn(backend, "subscribeRealtime").mockImplementation((handler) => {
    realtime = handler
    return () => { realtime = undefined }
  })

  // The canvas is a recording renderer: real painters run against a sized
  // surface. This catches missing/gapped geometry as well as DOM readouts.
  const contexts = new Map<HTMLCanvasElement, Record<string, unknown>>()
  let frameId = 0
  const paints = new Map<number, FrameRequestCallback>()
  vi.stubGlobal("requestAnimationFrame", (paint: FrameRequestCallback) => { paints.set(++frameId, paint); return frameId })
  vi.stubGlobal("cancelAnimationFrame", (id: number) => paints.delete(id))
  const paintFrame = () => {
    const pending = [...paints.values()]
    paints.clear()
    for (const paint of pending) paint(performance.now())
  }
  vi.spyOn(HTMLCanvasElement.prototype, "getBoundingClientRect").mockReturnValue({ x:0, y:0, top:0, left:0, right:320, bottom:120, width:320, height:120, toJSON:() => ({}) })
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(function (this: HTMLCanvasElement) {
    let context = contexts.get(this)
    if (!context) {
      context = Object.fromEntries(["setTransform", "clearRect", "beginPath", "moveTo", "lineTo", "stroke", "fill", "fillRect", "fillText", "closePath"].map((name) => [name, vi.fn()]))
      contexts.set(this, context)
    }
    return context as unknown as CanvasRenderingContext2D
  })

  const first = fixture.steps[0]
  if (!first.select || !first.answer) throw new Error("No initial native selection")
  answer = first.answer
  const view = render(<JoinedViews select={first.select} tracks={fixture.tracks} />)
  await act(async () => { await settle(); paintFrame(); await settle() })
  const visibleTicks: number[] = []
  const phases = new Set<string>()
  let limiterPeak = 0
  let sawDecay = false
  let sawTuner = false
  let sawNewTuner = false
  let sawRate = false
  let sawPdc = false
  let sawInvalid = false
  let sawReplacementTone = false
  let pdcBefore = 0
  let delayedDeletedSource: string | undefined
  for (const step of fixture.steps) {
    if (step.projectLoaded) {
      answer = step.answer
      await act(async () => {
        // The exact native project:loaded bytes enter the same store/reset
        // boundary connectStores uses, before any old analyzer callback.
        loadSnapshot(JSON.parse(step.projectLoaded!) as DocumentSnapshot)
        announceProjectReplaced()
        await settle()
        paintFrame()
        await settle()
      })
      await waitFor(() => expect(document.querySelector("[data-slot=analyzer-canvas]")).toHaveAttribute("data-live", "false"))
      await waitFor(() => expect(element("[data-slot=large-clock] [data-part=time]").textContent).toBe(formatClock(0)))
    }
    if (step.select && step !== first) {
      answer = step.answer
      view.rerender(<JoinedViews select={step.select} tracks={fixture.tracks} />)
      await act(() => settle())
    }
    for (const text of step.analyzer) {
      const event = JSON.parse(text) as AnalyzerEvent
      await act(async () => { analyzer?.(event); await settle(); paintFrame(); await settle() })
      phases.add(element("[data-slot=analyzer-status]").dataset.phase ?? "")
      if (event.type !== "frame") {
        if (event.phase !== "live") await waitFor(() => expect(datasets("scope-meter").live).toBeUndefined())
        continue
      }
      const frame: AnalyzerFrame = event
      if (step.name === "before-delete") delayedDeletedSource = text
      {
        expect(document.querySelector("[data-slot=analyzer-canvas]")).toHaveAttribute("data-live", "true")
        expect(document.querySelector("[data-slot=analyzer-canvas]")).toHaveAttribute("data-epoch", frame.clock.epoch)
        expect(element("[data-slot=scope-meter] [data-side=left]").dataset.peakDb).toBe(frame.levels[0].valid && !frame.levels[0].silent && frame.levels[0].peakDb !== null ? frame.levels[0].peakDb.toFixed(2) : "")
      }
      expect(BigInt(frame.clock.alignedFirstFrame)).toBe(BigInt(frame.clock.firstFrame) - BigInt(frame.clock.pdcFrames))
      for (let side = 0; side < 2; side += 1) {
        const row = element(`[data-slot=scope-meter] [data-side=${side === 0 ? "left" : "right"}]`)
        const level = frame.levels[side]
        expect(row.dataset.peakDb).toBe(level.valid && !level.silent && level.peakDb !== null ? level.peakDb.toFixed(2) : "")
        expect(row.hasAttribute("data-invalid")).toBe(!level.valid)
      }
      if (step.name === "left") {
        expect(frame.levels[0].peak).toBeGreaterThan(0.05)
        expect(frame.levels[1].peak).toBe(0)
        expect(Number(document.querySelector<HTMLElement>("[data-slot=analyzer-canvas]")?.dataset.peakHz)).toBeCloseTo(421.875, 0)
        const vectorCanvas = document.querySelector<HTMLCanvasElement>("canvas[aria-label^=Vectorscope]")
        const context = vectorCanvas ? contexts.get(vectorCanvas) : undefined
        expect(vi.mocked(context?.fillRect as ReturnType<typeof vi.fn>).mock.calls.some(([x, y]) => Number(x) < 160 && Number(y) < 60)).toBe(true)
      }
      if (step.name === "right") {
        expect(frame.levels[0].peak).toBe(0)
        expect(frame.levels[1].peak).toBeGreaterThan(0.05)
        expect(Number(document.querySelector<HTMLElement>("[data-slot=analyzer-canvas]")?.dataset.peakHz)).toBeCloseTo(1875, 0)
        const vectorCanvas = document.querySelector<HTMLCanvasElement>("canvas[aria-label^=Vectorscope]")
        const context = vectorCanvas ? contexts.get(vectorCanvas) : undefined
        expect(vi.mocked(context?.fillRect as ReturnType<typeof vi.fn>).mock.calls.some(([x, y]) => Number(x) > 160 && Number(y) < 60)).toBe(true)
      }
      if (step.name === "replacement" && frame.spectrum) {
        expect(Number(document.querySelector<HTMLElement>("[data-slot=analyzer-canvas]")?.dataset.peakHz)).toBeCloseTo(861.328125, 0)
        sawReplacementTone = true
      }
      if (frame.spectrum && step.name === "right") expect(Number(datasets("eq-live-spectrum").peakHz)).toBeGreaterThan(1700)
      for (const observation of frame.observations) {
        if (observation.type === "dynamics") {
          limiterPeak = Math.max(limiterPeak, observation.reductionDb)
          const last = frame.observations.filter((o) => o.type === "dynamics").at(-1)
          if (last?.type === "dynamics") {
            if (last.inputPeak > 0) expect(Number(datasets("limiter-history").inputDb)).toBeCloseTo(20 * Math.log10(last.inputPeak))
            else expect(Number(datasets("limiter-history").inputDb)).toBe(-Infinity)
            expect(Number(datasets("limiter-history").lastReductionDb)).toBeCloseTo(last.reductionDb)
            if (step.name === "decay" && last.reductionDb < limiterPeak / 2) sawDecay = true
          }
        }
        if (observation.type === "tuner" && observation === frame.observations.filter((o) => o.type === "tuner").at(-1) && observation.midiNote !== null) {
          await waitFor(() => expect(datasets("tuner-editor").cents).toBe(observation.cents?.toFixed(2)))
          if (step.name === "tuner-settled") {
            expect(observation.midiNote).toBe(69)
            expect(observation.cents).toBeCloseTo(1200 * Math.log2(440 / 432), 0)
            expect(element("[data-slot=tuner-note]")).toHaveTextContent("A4")
            sawTuner = true
          }
          if (step.name === "tuner-new-source") {
            expect(observation.frequencyHz).toBeCloseTo(1875, -1)
            sawNewTuner = true
          }
        }
      }
      if (step.name === "rate" && frame.clock.sampleRate === 44100) sawRate = true
      if (step.name === "pdc") pdcBefore = frame.clock.pdcFrames
      if (step.name === "pdc-change" && frame.clock.pdcFrames === pdcBefore + 480) sawPdc = true
      // Sample playback sanitizes non-finite PCM before the mixer tap. The
      // actual downstream meter must show valid silence, never NaN values.
      if (step.name === "invalid" && frame.levels.every((level) => level.valid && level.silent && level.peak === 0)) sawInvalid = true
    }
    for (const text of step.realtime) {
      const frame = JSON.parse(text) as RealtimeFrame
      await act(async () => { realtime?.(frame); await settle(); paintFrame(); await settle() })
      expect(element("[data-slot=large-clock] [data-part=position]").textContent).toBe(formatMusicalPosition(frame.tick, fixture.project.settings.timeSignature, []))
      expect(element("[data-slot=large-clock] [data-part=time]").textContent).toBe(formatClock(ticksToSeconds(frame.tick, fixture.project.settings.tempoBpm)))
      expect(element("[data-slot=large-master-meter] [data-side=left]").textContent).toBe((frame.meters[0] ?? 0) === 0 ? "−∞ dBFS" : `${(20 * Math.log10(Math.abs(frame.meters[0]))).toFixed(1)} dBFS`)
      if (step.name === "loop") visibleTicks.push(frame.tick)
      for (const waveform of frame.waveforms ?? []) {
        const canvas = element(`[data-slot=mixer-waveform][data-track="${waveform.track}"]`)
        expect(canvas.dataset.serial).toBe(String(waveform.serial))
      }
      if (step.name === "left") {
        await waitFor(() => expect(Number(element(`[data-track="${fixture.tracks[0]}"]`).dataset.leftPeak)).toBeGreaterThan(0.05))
        expect(Number(element(`[data-track="${fixture.tracks[0]}"]`).dataset.rightPeak)).toBe(0)
      }
    }
    if (step.name === "deleted" || step.name === "project-replaced" || step.name === "audio-stall") {
      await waitFor(() => expect(document.querySelector("[data-slot=analyzer-canvas]")).toHaveAttribute("data-live", "false"))
      expect(datasets("limiter-history").blocks).toBe("0")
      expect(element("[data-slot=tuner-note]")).toHaveTextContent("–")
      expect(datasets("eq-live-spectrum").peakHz).toBeUndefined()
      if (step.name === "deleted" && delayedDeletedSource) {
        // A real captured predecessor, still byte-for-byte intact, arriving
        // late from the channel cannot restore a terminally refused source.
        await act(async () => { analyzer?.(JSON.parse(delayedDeletedSource!) as AnalyzerEvent); await settle() })
        expect(document.querySelector("[data-slot=analyzer-canvas]")).toHaveAttribute("data-live", "false")
        expect(datasets("eq-live-spectrum").peakHz).toBeUndefined()
      }
    }
  }
  expect(visibleTicks.some((tick, index) => index > 0 && tick < visibleTicks[index - 1])).toBe(true)
  expect(limiterPeak).toBeGreaterThan(3)
  expect({ sawDecay, sawTuner, sawNewTuner, sawRate, sawPdc, sawInvalid, sawReplacementTone }).toEqual({ sawDecay: true, sawTuner: true, sawNewTuner: true, sawRate: true, sawPdc: true, sawInvalid: true, sawReplacementTone: true })
  expect(phases.has("stalled")).toBe(true)
  expect(backend.analyzerAck).toHaveBeenCalled()
  expect([...contexts.values()].some((context) => vi.mocked(context.lineTo as ReturnType<typeof vi.fn>).mock.calls.length > 0)).toBe(true)
})
