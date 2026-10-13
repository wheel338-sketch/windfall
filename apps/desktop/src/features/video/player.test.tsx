import { act, cleanup, render, screen, waitFor } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import type { MediaClock, RealtimeFrame, VideoPlacement } from "@/bindings"
import { useProjectStore } from "@/lib/store/project"
import { starterProject } from "@/lib/ipc/sim/project"
import { startTestApp } from "@/test/harness"
import { VideoPlayer } from "./player"

// Independently authored four-frame BI_RGB/PCM file, not readRawAvi output or
// a stub of the picture decoder. The recording context below checks what the
// mounted component submits; real raster/readback evidence lives in the
// Windows smoke because this repository's jsdom has no canvas implementation.
function authoredAvi(): ArrayBuffer {
  const join = (parts: Uint8Array[]) => {
    const bytes = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0))
    let at = 0
    for (const part of parts) { bytes.set(part, at); at += part.length }
    return bytes
  }
  const text = (value: string) => new TextEncoder().encode(value)
  const chunk = (id: string, data: Uint8Array) => {
    const header = new Uint8Array(8); header.set(text(id))
    new DataView(header.buffer).setUint32(4, data.length, true)
    return join([header, data, new Uint8Array(data.length % 2)])
  }
  const list = (id: string, parts: Uint8Array[]) => chunk("LIST", join([text(id), ...parts]))
  const stream = (kind: string, rate: number) => {
    const data = new Uint8Array(56); data.set(text(kind))
    const view = new DataView(data.buffer)
    view.setUint32(20, 1, true); view.setUint32(24, rate, true); view.setUint32(32, 4, true)
    return chunk("strh", data)
  }
  const dib = new Uint8Array(40), d = new DataView(dib.buffer)
  d.setUint32(0, 40, true); d.setInt32(4, 2, true); d.setInt32(8, 2, true)
  d.setUint16(12, 1, true); d.setUint16(14, 24, true)
  const wave = new Uint8Array(16), w = new DataView(wave.buffer)
  w.setUint16(0, 1, true); w.setUint16(2, 1, true); w.setUint32(4, 4, true)
  w.setUint32(8, 8, true); w.setUint16(12, 2, true); w.setUint16(14, 16, true)
  const frames = Array.from({ length: 4 }, (_, frame) => {
    const rgb = new Uint8Array(16)
    for (let y = 0; y < 2; y += 1) for (let x = 0; x < 2; x += 1) {
      rgb.set([200 - frame, 30 + x + y * 10, frame + 1], (1 - y) * 8 + x * 3)
    }
    return chunk("00db", rgb)
  })
  return chunk("RIFF", join([text("AVI "), list("hdrl", [
    list("strl", [stream("vids", 4), chunk("strf", dib)]),
    list("strl", [stream("auds", 4), chunk("strf", wave)])]),
    list("movi", [...frames, chunk("01wb", new Uint8Array([0, 16, 0, 32, 0, 48, 0, 64]))])])).buffer
}

let stop: (() => void) | undefined
afterEach(() => { cleanup(); stop?.(); stop = undefined; vi.restoreAllMocks(); vi.unstubAllGlobals() })

it("mounts the real raw AVI player, paints exact frames at offset/seek/loop and clears Stop before repainting on resume", async () => {
  const app = await startTestApp({ project: starterProject() }); stop = app.stop
  let deliver!: (frame: RealtimeFrame) => void
  vi.spyOn(app.backend, "subscribeRealtime").mockImplementation((listener) => { deliver = listener; return () => {} })
  vi.spyOn(app.backend, "mediaVideoRead").mockResolvedValue(authoredAvi())
  vi.spyOn(app.backend, "timelineState").mockResolvedValue({ generation: 1,
    revision: useProjectStore.getState().revision, request: 0, region: null, navigationOverflows: 0 })
  const callbacks = new Map<number, FrameRequestCallback>(); let next = 0
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { callbacks.set(++next, callback); return next })
  vi.stubGlobal("cancelAnimationFrame", (id: number) => { callbacks.delete(id) })
  const image = { width: 2, height: 2, data: new Uint8ClampedArray(16), colorSpace: "srgb" } as ImageData
  const put = vi.fn(), clear = vi.fn()
  const context = { createImageData: () => image, putImageData: put, clearRect: clear }
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(context as unknown as CanvasRenderingContext2D)
  const placement: VideoPlacement = { path: "C:/authored.avi", startTick: 960, lengthTicks: 1920,
    offsetSeconds: 0.25, muted: false, mixerTrack: 0 }
  render(<VideoPlayer placement={placement} />)
  const canvas = await screen.findByRole("img", { name: "Timeline video" })
  await waitFor(() => expect(app.backend.mediaVideoRead).toHaveBeenCalled())
  const paint = (tick: number, playing = true, epoch = "1") => act(() => {
    const clock: MediaClock = { epoch, outputFrame: String(++next), sampleRate: 48000,
      pdcFrames: 0, deviceLatencyFrames: null, playing, mode: "song", tick }
    deliver({ playing, tick, mediaClock: clock, meters: [0.125, 0.25], cpu: 0, xruns: 0,
      voices: 0, gainReductions: [], automated: [], audioClips: 0, droppedClips: 0 })
    const due = [...callbacks.values()]; callbacks.clear()
    for (const callback of due) callback(0)
  })
  const assertFrame = (frame: number, seconds: number) => {
    expect(canvas).toHaveAttribute("data-frame-index", String(frame))
    expect(Number(canvas.getAttribute("data-source-seconds"))).toBeCloseTo(seconds, 10)
    const expected = []
    for (let y = 0; y < 2; y += 1) for (let x = 0; x < 2; x += 1) expected.push(frame + 1, 30 + x + y * 10, 200 - frame, 255)
    expect([...image.data]).toEqual(expected)
    expect(put).toHaveBeenLastCalledWith(image, 0, 0)
  }
  paint(960); assertFrame(1, 0.25)
  paint(1440); assertFrame(2, 0.5)
  paint(1920, true, "2"); assertFrame(3, 0.75)
  paint(960, true, "2"); assertFrame(1, 0.25)
  paint(0, false, "3")
  expect(canvas).toHaveAttribute("data-frame-index", "-1")
  expect(clear).toHaveBeenLastCalledWith(0, 0, 2, 2)
  expect(canvas).toHaveAttribute("data-source-seconds", "")
  // Stop while the cursor is *inside* the clip must clear too. Previously this
  // repainted a first frame because the picture path ignored clock.playing.
  paint(960); assertFrame(1, 0.25)
  paint(960, false, "4")
  expect(canvas).toHaveAttribute("data-frame-index", "-1")
  const writes = put.mock.calls.length
  paint(960, false, "4"); expect(put).toHaveBeenCalledTimes(writes)
  paint(960, true, "5"); assertFrame(1, 0.25)
  paint(2880); expect(canvas).toHaveAttribute("data-frame-index", "-1")
})

it("hides the HTML decoded picture on Stop while preserving native audio ownership", async () => {
  const app = await startTestApp({ project: starterProject() }); stop = app.stop
  let deliver!: (frame: RealtimeFrame) => void
  vi.spyOn(app.backend, "subscribeRealtime").mockImplementation((listener) => { deliver = listener; return () => {} })
  vi.spyOn(app.backend, "mediaVideoRead").mockResolvedValue(new ArrayBuffer(16))
  vi.spyOn(app.backend, "timelineState").mockResolvedValue({ generation: 1,
    revision: useProjectStore.getState().revision, request: 0, region: null, navigationOverflows: 0 })
  const revoke = vi.fn()
  vi.stubGlobal("URL", { createObjectURL: () => "blob:authored-html-video", revokeObjectURL: revoke })
  let draw: FrameRequestCallback | undefined
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { draw = callback; return 1 })
  vi.stubGlobal("cancelAnimationFrame", () => {})
  const placement: VideoPlacement = { path: "C:/authored.webm", startTick: 0, lengthTicks: 1920,
    offsetSeconds: 0, muted: false, mixerTrack: 0 }
  const mounted = render(<VideoPlayer placement={placement} />)
  const video = screen.getByLabelText("Timeline video") as HTMLVideoElement
  await waitFor(() => expect(video).toHaveAttribute("src", "blob:authored-html-video"))
  Object.defineProperties(video, { readyState: { value: 4, configurable: true },
    duration: { value: 10, configurable: true }, paused: { value: true, configurable: true } })
  const play = vi.spyOn(video, "play").mockResolvedValue()
  const pause = vi.spyOn(video, "pause").mockImplementation(() => {})
  const paint = (playing: boolean, epoch: string) => act(() => {
    deliver({ playing, tick: 480, mediaClock: { epoch, outputFrame: epoch, sampleRate: 48000,
      pdcFrames: 0, deviceLatencyFrames: null, playing, mode: "song", tick: 480 },
    meters: [], cpu: 0, xruns: 0, voices: 0, gainReductions: [], automated: [], audioClips: 0, droppedClips: 0 })
    draw?.(0)
  })
  paint(true, "1"); expect(video.style.visibility).toBe("visible")
  expect(video.muted).toBe(true); expect(play).toHaveBeenCalledTimes(1)
  paint(false, "2"); expect(video.style.visibility).toBe("hidden")
  expect(pause).toHaveBeenCalled(); expect(play).toHaveBeenCalledTimes(1)
  mounted.unmount(); expect(revoke).toHaveBeenCalledWith("blob:authored-html-video")
})
