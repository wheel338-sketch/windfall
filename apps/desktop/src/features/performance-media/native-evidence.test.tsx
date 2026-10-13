/// <reference types="node" />
import { Blob as NodeBlob } from "node:buffer"
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"

import type { DocumentSnapshot, MediaRenderAudio, Project, RealtimeFrame, TickRange, TimelinePlaybackState } from "@/bindings"
import { resetAnalyzerFeed } from "@/features/analyzers/feed"
import { NoteCharacter } from "@/features/visualizer/character"
import { VisualizerPanel } from "@/features/visualizer/panel"
import { loadSnapshot, useProjectStore } from "@/lib/store/project"
import { announceProjectReplaced } from "@/lib/store/replaced"
import { settle, startTestApp } from "@/test/harness"
import { useMediaWindows } from "./store"

type Capture = {
  schema: number
  project: Project
  selected: number
  other: number
  steps: { name: string; realtime: string; projectLoaded?: string }[]
  range: TickRange
  audio: MediaRenderAudio
  renderProject: Project
  renderGuard: TimelinePlaybackState
}

function capture(): Capture {
  // This file can only be produced by the registered device-free native test.
  // A missing capture fails; it must never become authored note poses or PCM.
  const file = process.env.WINDFALL_MEDIA_CAPTURE_DIR
    ? resolve(import.meta.dirname, "../../../../..", process.env.WINDFALL_MEDIA_CAPTURE_DIR, "native-media.json")
    : resolve(import.meta.dirname, "fixtures/native-media.json")
  const result = JSON.parse(readFileSync(file, "utf8")) as Capture
  expect(result.schema).toBe(1)
  expect(result.audio).toMatchObject({ sampleRate: 48000, channels: 2 })
  expect(result.audio.samples).toHaveLength(24025 * 2)
  expect(result.range).toEqual({ start: 960, end: 1921 })
  return result
}

let stop: (() => void) | undefined
let folder: string | undefined
afterEach(() => {
  useMediaWindows.setState({ performance: false, video: false, visualizer: false })
  resetAnalyzerFeed()
  stop?.()
  stop = undefined
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  if (folder) rmSync(folder, { recursive: true, force: true })
  folder = undefined
})

function animationFrames() {
  let serial = 0
  const callbacks = new Map<number, FrameRequestCallback>()
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callbacks.set(++serial, callback)
    return serial
  })
  vi.stubGlobal("cancelAnimationFrame", (id: number) => callbacks.delete(id))
  return () => {
    const pending = [...callbacks.values()]
    callbacks.clear()
    for (const callback of pending) callback(performance.now())
  }
}

it("delivers actual sequenced and live native note on/off to mounted Moss with channel filtering and replacement clearing", async () => {
  const fixture = capture()
  const app = await startTestApp()
  stop = app.stop
  useProjectStore.setState({ project: fixture.project, ready: true })
  let receive: ((frame: RealtimeFrame) => void) | undefined
  vi.spyOn(app.backend, "subscribeRealtime").mockImplementation((handler) => {
    receive = handler
    return () => { receive = undefined }
  })
  const paint = animationFrames()
  const view = render(<NoteCharacter channel={fixture.selected} />)
  const expected: Record<string, string> = {
    rest: "rest", "sequenced-on": "step", "sequenced-off": "rest",
    "other-live-on": "rest", "selected-live-on": "jump", "selected-live-off": "rest",
    "other-live-off": "rest", "before-replacement": "lean", replacement: "rest",
  }
  expect(fixture.steps.map((step) => step.name)).toEqual(Object.keys(expected))
  for (const step of fixture.steps) {
    if (step.projectLoaded) {
      await act(async () => {
        loadSnapshot(JSON.parse(step.projectLoaded!) as DocumentSnapshot)
        announceProjectReplaced()
        paint()
        await settle()
      })
      // Clearing is immediate at the frontend replacement boundary, before
      // the replacement processor's next realtime publication arrives.
      expect(document.querySelector("[data-pose]")).toHaveAttribute("data-pose", "rest")
    }
    const frame = JSON.parse(step.realtime) as RealtimeFrame
    expect(receive).toBeDefined()
    await act(async () => { receive!(frame); paint(); await settle() })
    expect(document.querySelector("[data-pose]")).toHaveAttribute("data-pose", expected[step.name])
    if (step.name === "other-live-on") {
      // The same intact native publication animates the other channel, but
      // switching back to the saved selection must immediately rest.
      view.rerender(<NoteCharacter channel={fixture.other} />)
      await act(async () => { paint(); await settle() })
      expect(document.querySelector("[data-pose]")).toHaveAttribute("data-pose", "wave")
      view.rerender(<NoteCharacter channel={fixture.selected} />)
      await act(async () => { paint(); await settle() })
      expect(document.querySelector("[data-pose]")).toHaveAttribute("data-pose", "rest")
    }
    if (step.name === "selected-live-on") {
      expect(document.querySelector("[data-limb=left-arm]")).toHaveAttribute("transform", "rotate(130)")
      expect(screen.getByRole("img", { name: "Moss note character" }).querySelector("title")).toHaveTextContent("note 62")
    }
    if (step.name === "selected-live-off") {
      expect(frame.activeNotePoses?.some((note) => note.channel === fixture.other)).toBe(true)
      expect(screen.getByRole("img", { name: "Moss note character" }).querySelector("title")).toHaveTextContent("Moss: rest")
    }
  }
})

type Chunk = { id: string; at: number; size: number; kind?: string; children: Chunk[] }
const fourcc = (bytes: Uint8Array, at: number) => String.fromCharCode(...bytes.subarray(at, at + 4))
// Independent RIFF reader: no writer or metadata helper is used as an oracle.
function chunks(bytes: Uint8Array, from: number, end: number): Chunk[] {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const found: Chunk[] = []
  let at = from
  while (at < end) {
    expect(at + 8).toBeLessThanOrEqual(end)
    const id = fourcc(bytes, at)
    const size = view.getUint32(at + 4, true)
    expect(at + 8 + size).toBeLessThanOrEqual(end)
    const container = id === "RIFF" || id === "LIST"
    found.push({ id, at, size, kind: container ? fourcc(bytes, at + 8) : undefined,
      children: container ? chunks(bytes, at + 12, at + 8 + size) : [] })
    at += 8 + size + size % 2
  }
  expect(at).toBe(end)
  return found
}

function assertPublishedAvi(file: string, audio: MediaRenderAudio) {
  const bytes = readFileSync(file)
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const roots = chunks(bytes, 0, bytes.length)
  expect(roots).toHaveLength(1)
  const root = roots[0]
  expect(root).toMatchObject({ id: "RIFF", kind: "AVI ", size: bytes.length - 8 })
  const header = root.children.find((entry) => entry.kind === "hdrl")!
  const streams = header.children.filter((entry) => entry.kind === "strl")
  expect(streams).toHaveLength(2)
  const frameCount = Math.ceil(24025 * 24 / 48000)
  const avih = header.children.find((entry) => entry.id === "avih")!.at + 8
  expect(view.getUint32(avih + 16, true)).toBe(frameCount)
  expect(view.getUint32(avih + 24, true)).toBe(2)
  const video = streams[0].children.find((entry) => entry.id === "strh")!.at + 8
  expect(fourcc(bytes, video)).toBe("vids")
  expect(fourcc(bytes, video + 4)).toBe("DIB ")
  expect(view.getUint32(video + 20, true)).toBe(1)
  expect(view.getUint32(video + 24, true)).toBe(24)
  expect(view.getUint32(video + 32, true)).toBe(frameCount)
  const videoFormat = streams[0].children.find((entry) => entry.id === "strf")!.at + 8
  expect(view.getInt32(videoFormat + 4, true)).toBe(320)
  expect(view.getInt32(videoFormat + 8, true)).toBe(180)
  expect(view.getUint16(videoFormat + 14, true)).toBe(24)
  expect(view.getUint32(videoFormat + 16, true)).toBe(0)
  const sound = streams[1].children.find((entry) => entry.id === "strh")!.at + 8
  expect(fourcc(bytes, sound)).toBe("auds")
  expect(view.getUint32(sound + 20, true)).toBe(4)
  expect(view.getUint32(sound + 24, true)).toBe(48000 * 4)
  expect(view.getUint32(sound + 32, true)).toBe(24025)
  expect(view.getUint32(sound + 44, true)).toBe(4)
  const soundFormat = streams[1].children.find((entry) => entry.id === "strf")!.at + 8
  expect(view.getUint16(soundFormat, true)).toBe(1)
  expect(view.getUint16(soundFormat + 2, true)).toBe(2)
  expect(view.getUint32(soundFormat + 4, true)).toBe(48000)
  expect(view.getUint32(soundFormat + 8, true)).toBe(192000)
  expect(view.getUint16(soundFormat + 12, true)).toBe(4)
  expect(view.getUint16(soundFormat + 14, true)).toBe(16)
  const movie = root.children.find((entry) => entry.kind === "movi")!
  const frames = movie.children.filter((entry) => entry.id === "00db")
  expect(frames).toHaveLength(frameCount)
  expect(frames.every((entry) => entry.size === 320 * 180 * 3)).toBe(true)
  const pictures = frames.map((frame, index) => {
    let foreground = 0
    for (let at = frame.at + 8; at < frame.at + 8 + frame.size; at += 3) {
      if (bytes[at] !== 20 || bytes[at + 1] !== 12 || bytes[at + 2] !== 8) foreground += 1
    }
    // Frame zero has no completed 1024-sample analyzer window. The next
    // frame starts at 2000 samples (48000 / 24), so all later frames do.
    if (index === 0) expect(foreground).toBe(0)
    else expect(foreground).toBeGreaterThan(0)
    return bytes.subarray(frame.at + 8, frame.at + 8 + frame.size)
  })
  expect(pictures.some((picture) => !picture.equals(pictures[0]))).toBe(true)
  const pcm = movie.children.filter((entry) => entry.id === "01wb")
  expect(pcm.reduce((sum, entry) => sum + entry.size, 0)).toBe(24025 * 4)
  expect(pcm.at(-1)?.size).toBe(25 * 4)
  let sample = 0
  for (const chunk of pcm) for (let at = chunk.at + 8; at < chunk.at + 8 + chunk.size; at += 2) {
    const original = audio.samples[sample++]
    const expected = Math.round(Math.max(-1, Math.min(1, original)) * (original < 0 ? 32768 : 32767))
    expect(view.getInt16(at, true)).toBe(expected || 0)
  }
  expect(sample).toBe(audio.samples.length)
  const index = root.children.find((entry) => entry.id === "idx1")!
  expect(index.size).toBe(movie.children.length * 16)
  for (const [n, chunk] of movie.children.entries()) {
    const at = index.at + 8 + n * 16
    expect(fourcc(bytes, at)).toBe(chunk.id)
    expect(movie.at + 8 + view.getUint32(at + 8, true)).toBe(chunk.at)
    expect(view.getUint32(at + 12, true)).toBe(chunk.size)
  }
}

it("exports synchronized AVI through the enabled mounted action using native region PCM and cancels without publishing a partial file", async () => {
  const fixture = capture()
  const app = await startTestApp()
  stop = app.stop
  // Only transport is substituted: the real native renderer's captured result
  // enters the real UI action, rasterizer, AVI encoder and download publisher.
  Object.defineProperty(app.backend, "kind", { value: "tauri", configurable: true })
  useProjectStore.setState({ project: fixture.renderProject, revision: fixture.renderGuard.revision, ready: true })
  vi.spyOn(app.backend, "timelineState").mockResolvedValue(fixture.renderGuard)
  const renderAudio = vi.spyOn(app.backend, "mediaRenderAudio").mockResolvedValue(fixture.audio)
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null)
  const paint = animationFrames()
  folder = mkdtempSync(join(tmpdir(), "windfall-media-publication-"))
  const urls = new Map<string, NodeBlob>()
  const writes: Promise<void>[] = []
  let serial = 0
  const create = vi.fn((blob: Blob | MediaSource) => {
    if (!(blob instanceof NodeBlob)) throw new Error("Publication must own real Blob bytes")
    const url = `blob:media-${++serial}`
    urls.set(url, blob)
    return url
  })
  const revoke = vi.fn((url: string) => { urls.delete(url) })
  vi.stubGlobal("Blob", NodeBlob)
  vi.stubGlobal("URL", class extends URL { static createObjectURL = create; static revokeObjectURL = revoke })
  vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
    const blob = urls.get(this.href)
    if (!blob || !folder) throw new Error("Download did not publish owned bytes")
    const file = join(folder, this.download)
    writes.push(blob.arrayBuffer().then((bytes) => { writeFileSync(file, new Uint8Array(bytes)) }))
  })
  render(<VisualizerPanel />)
  await act(async () => { useMediaWindows.setState({ visualizer: true }); await settle() })
  const user = userEvent.setup()
  const start = screen.getByRole("spinbutton", { name: "Export start tick" })
  await user.clear(start); await user.type(start, String(fixture.range.start)); await user.tab()
  const end = screen.getByRole("spinbutton", { name: "Export end tick" })
  await user.clear(end); await user.type(end, String(fixture.range.end)); await user.tab()
  const action = screen.getByRole("button", { name: "Export synchronized AVI" })
  expect(action).toBeEnabled()
  await user.click(action)
  await waitFor(() => expect(screen.getByRole("button", { name: "Cancel export" })).toBeVisible())
  expect(renderAudio).toHaveBeenCalledWith(fixture.range, fixture.renderGuard.generation, fixture.renderGuard.revision)
  // The encoder yields at frame 8; a real rendering-lifecycle callback must
  // run before all 13 native-synchronized video frames can be published.
  expect(create).not.toHaveBeenCalled()
  await act(async () => { paint(); await settle() })
  await waitFor(() => expect(action).toBeEnabled())
  await Promise.all(writes)
  expect(readdirSync(folder)).toEqual(["windfall-visual.avi", "windfall-visual.json"])
  assertPublishedAvi(join(folder, "windfall-visual.avi"), fixture.audio)
  const metadata = JSON.parse(readFileSync(join(folder, "windfall-visual.json"), "utf8"))
  expect(metadata).toMatchObject({ range: fixture.range, audioFrames: 24025, videoFrames: 13, firstVisualFrame: 12, sampleRate: 48000 })
  await act(async () => { paint(); await settle() })
  expect(revoke).toHaveBeenCalledTimes(2)
  const accepted = readFileSync(join(folder, "windfall-visual.avi"))
  const published = create.mock.calls.length
  await user.click(action)
  await waitFor(() => expect(screen.getByRole("button", { name: "Cancel export" })).toBeVisible())
  expect(create).toHaveBeenCalledTimes(published)
  await user.click(screen.getByRole("button", { name: "Cancel export" }))
  await act(async () => { paint(); await settle() })
  await waitFor(() => expect(action).toBeEnabled())
  expect(create).toHaveBeenCalledTimes(published)
  expect(writes).toHaveLength(2)
  expect(readdirSync(folder)).toEqual(["windfall-visual.avi", "windfall-visual.json"])
  // Buffer.equals compares length and every byte; generic object equality
  // traverses millions of indexed properties for this uncompressed file.
  expect(readFileSync(join(folder, "windfall-visual.avi")).equals(accepted)).toBe(true)
})
