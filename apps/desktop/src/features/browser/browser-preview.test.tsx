import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"

import type { SampleInfo } from "@/bindings"
import type { Backend } from "@/lib/ipc"

import BrowserPanel from "."
import { readPersisted } from "./persist"
import { REPEAT_SETTLE_MS } from "./preview"
import { useBrowserStore } from "./store"
import { deferred, findItem, item, startBrowserTest, tree } from "./testing"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let stop: () => void = () => undefined
afterEach(() => stop())

const KICKS = "/factory/Drums/Kicks"

/** Renders the panel with Drums and Kicks open and nothing selected yet. */
async function openKicks(override?: (mock: Backend) => Partial<Backend>) {
  const user = userEvent.setup()
  const app = await startBrowserTest(override)
  stop = app.stop
  render(<BrowserPanel />)
  await user.click(await findItem("Drums"))
  await user.click(await findItem("Kicks"))
  await findItem("Kick 01.wav")
  return { user, backend: app.backend }
}

const press = (key: string, init: KeyboardEventInit = {}) =>
  fireEvent.keyDown(tree(), { key, ...init })

const pane = () => screen.getByRole("region", { name: "Preview" })

function infoFor(name: string, durationSecs: number): SampleInfo {
  return {
    path: `${KICKS}/${name}`,
    name,
    sampleRate: 48_000,
    channels: 2,
    frames: Math.round(durationSecs * 48_000),
    durationSecs,
    peaks: [-0.5, 0.5, -0.25, 0.25],
  }
}

describe("previewing", () => {
  it("plays a sound the moment it is pressed, and again on every press", async () => {
    const { backend } = await openKicks()
    const play = vi.spyOn(backend, "previewPlay")

    // The press alone does it; there is no waiting for the release.
    fireEvent.mouseDown(item("Kick 02.wav"))
    expect(play.mock.calls.map(([path]) => [path])).toEqual([
      [`${KICKS}/Kick 02.wav`],
    ])
    expect(item("Kick 02.wav")).toHaveAttribute("aria-selected", "true")

    await act(async () => {})
    fireEvent.mouseDown(item("Kick 02.wav"))
    expect(play).toHaveBeenCalledTimes(2)
  })

  it("plays each sound the arrow keys land on", async () => {
    const { backend } = await openKicks()
    const play = vi.spyOn(backend, "previewPlay")

    press("ArrowDown")
    await act(async () => {})
    press("ArrowDown")
    await act(async () => {})
    press("ArrowUp")
    await act(async () => {})

    expect(play.mock.calls.map(([path]) => path)).toEqual([
      `${KICKS}/Kick 01.wav`,
      `${KICKS}/Kick 02.wav`,
      `${KICKS}/Kick 01.wav`,
    ])
  })

  it("does not play when the selection lands on a folder", async () => {
    const { backend } = await openKicks()
    const play = vi.spyOn(backend, "previewPlay")
    press("ArrowUp")
    press("Home")
    expect(play).not.toHaveBeenCalled()
  })

  it("sends one call at a time and skips sounds the selection has left", async () => {
    const calls: { path: string; done: () => void }[] = []
    await openKicks(() => ({
      previewPlay: (path) => {
        const gate = deferred<void>()
        calls.push({ path, done: () => gate.resolve() })
        return gate.promise
      },
    }))
    const sent = () => calls.map((call) => call.path)

    press("ArrowDown")
    press("ArrowDown")
    press("ArrowDown")
    // The engine is still busy with the first one. Nothing is queued behind.
    expect(sent()).toEqual([`${KICKS}/Kick 01.wav`])

    await act(async () => calls[0].done())
    // Kick 02 was passed over: only where the selection is now gets played.
    expect(sent()).toEqual([`${KICKS}/Kick 01.wav`, `${KICKS}/Kick 03.wav`])
    // The engine has not answered for the newest one, so the one that was
    // replaced is not shown as playing.
    expect(useBrowserStore.getState().playing).toBeNull()

    await act(async () => calls[1].done())
    expect(useBrowserStore.getState().playing?.path).toBe(
      `${KICKS}/Kick 03.wav`
    )
  })

  it("plays nothing while an arrow key is held, then where it stops", async () => {
    vi.useFakeTimers()
    try {
      const { backend } = await openKicksWithFakeTimers()
      const play = vi.spyOn(backend, "previewPlay")
      const info = vi.spyOn(backend, "sampleInfo")

      press("ArrowDown")
      expect(play).toHaveBeenCalledTimes(1)
      press("ArrowDown", { repeat: true })
      press("ArrowDown", { repeat: true })
      await act(async () => {
        await vi.advanceTimersByTimeAsync(REPEAT_SETTLE_MS - 10)
      })
      press("ArrowUp", { repeat: true })
      expect(play).toHaveBeenCalledTimes(1)
      expect(item("Kick 02.wav")).toHaveAttribute("aria-selected", "true")

      await act(async () => {
        await vi.advanceTimersByTimeAsync(REPEAT_SETTLE_MS)
      })
      expect(play.mock.calls.map(([path]) => path)).toEqual([
        `${KICKS}/Kick 01.wav`,
        `${KICKS}/Kick 02.wav`,
      ])
      // The same holds for reading waveforms: the first and the last only.
      expect(info.mock.calls.map(([path]) => path)).toEqual([
        `${KICKS}/Kick 01.wav`,
        `${KICKS}/Kick 02.wav`,
      ])
    } finally {
      vi.useRealTimers()
    }
  })

  it("stops with the stop button and with Escape", async () => {
    const { user, backend } = await openKicks()
    const stopPreview = vi.spyOn(backend, "previewStop")

    await user.click(item("Kick 01.wav"))
    await waitFor(() =>
      expect(useBrowserStore.getState().playing).not.toBeNull()
    )
    await user.click(screen.getAllByRole("button", { name: "Stop preview" })[0])
    expect(stopPreview).toHaveBeenCalledTimes(1)
    expect(useBrowserStore.getState().playing).toBeNull()

    await user.click(item("Kick 01.wav"))
    await user.keyboard("{Escape}")
    await waitFor(() => expect(stopPreview).toHaveBeenCalledTimes(2))
  })

  it("stops after a play that was still under way", async () => {
    const gate = deferred<void>()
    const order: string[] = []
    await openKicks(() => ({
      previewPlay: () => {
        order.push("play")
        return gate.promise
      },
      previewStop: () => {
        order.push("stop")
        return Promise.resolve()
      },
    }))

    fireEvent.mouseDown(item("Kick 01.wav"))
    fireEvent.keyDown(tree(), { key: "Escape" })
    expect(order).toEqual(["play"])
    await act(async () => gate.resolve())
    // The stop is sent after the play has landed, so it cannot be undone
    // by it, and the sound is never shown as playing.
    expect(order).toEqual(["play", "stop"])
    expect(useBrowserStore.getState().playing).toBeNull()
  })

  it("shows why a sound could not be played", async () => {
    const { user } = await openKicks(() => ({
      previewPlay: () => Promise.reject(new Error("Unsupported codec")),
    }))
    await user.click(item("Kick 01.wav"))
    expect(
      await screen.findByText("Could not play it. Unsupported codec")
    ).toBeInTheDocument()
  })
})

describe("auto-preview", () => {
  const toggle = () =>
    screen.getByRole("button", { name: "Preview sounds when selected" })

  it("is on at first, and off means selecting is silent", async () => {
    const { user, backend } = await openKicks()
    const play = vi.spyOn(backend, "previewPlay")
    expect(toggle()).toHaveAttribute("aria-pressed", "true")

    await user.click(toggle())
    expect(toggle()).toHaveAttribute("aria-pressed", "false")
    expect(readPersisted().autoPreview).toBe(false)

    await user.click(item("Kick 01.wav"))
    press("ArrowDown")
    expect(item("Kick 02.wav")).toHaveAttribute("aria-selected", "true")
    expect(play).not.toHaveBeenCalled()

    // The pane still shows the sound, and can play it on request.
    expect(await screen.findByText("0.55 s")).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Play preview" }))
    expect(play.mock.calls.map(([path]) => [path])).toEqual([
      [`${KICKS}/Kick 02.wav`],
    ])
  })

  it("stops what is playing when it is turned off", async () => {
    const { user, backend } = await openKicks()
    const stopPreview = vi.spyOn(backend, "previewStop")
    await user.click(item("Kick 01.wav"))
    await user.click(toggle())
    expect(stopPreview).toHaveBeenCalledTimes(1)
  })

  it("stays off after a restart", async () => {
    const { user } = await openKicks()
    await user.click(toggle())
    stop()

    const app = await startBrowserTest()
    stop = app.stop
    expect(useBrowserStore.getState().autoPreview).toBe(false)
  })
})

describe("the preview pane", () => {
  it("shows the selected sound's facts, and a hint when no sound is selected", async () => {
    const { user } = await openKicks()
    expect(pane()).toHaveTextContent("Select a sound to hear it.")

    await user.click(item("Kick 01.wav"))
    expect(await screen.findByText("0.42 s")).toBeInTheDocument()
    expect(pane()).toHaveTextContent("Kick 01.wav")
    expect(pane()).toHaveTextContent("44.1 kHz")
    expect(pane()).toHaveTextContent("Mono")
    expect(
      screen.getByRole("img", { name: "Waveform of Kick 01.wav" })
    ).toBeInTheDocument()

    press("ArrowUp")
    expect(pane()).toHaveTextContent("Select a sound to hear it.")
  })

  it("never shows one sound's waveform under another's name", async () => {
    const asked: { path: string; answer: (info: SampleInfo) => void }[] = []
    await openKicks(() => ({
      sampleInfo: (path) => {
        const gate = deferred<SampleInfo>()
        asked.push({ path, answer: gate.resolve })
        return gate.promise
      },
    }))

    press("ArrowDown")
    press("ArrowDown")
    press("ArrowDown")
    // One read is under way, for the first sound. The pane is on the third.
    expect(asked.map((ask) => ask.path)).toEqual([`${KICKS}/Kick 01.wav`])
    expect(pane()).toHaveTextContent("Kick 03.wav")
    expect(
      screen.getByRole("status", { name: "Reading the sound" })
    ).toBeInTheDocument()

    // The late answer for the first sound must not reach the pane.
    await act(async () => asked[0].answer(infoFor("Kick 01.wav", 1.11)))
    expect(pane()).not.toHaveTextContent("1.11 s")
    expect(screen.queryByRole("img")).not.toBeInTheDocument()
    // The second sound was passed over and is never read.
    expect(asked.map((ask) => ask.path)).toEqual([
      `${KICKS}/Kick 01.wav`,
      `${KICKS}/Kick 03.wav`,
    ])

    await act(async () => asked[1].answer(infoFor("Kick 03.wav", 3.33)))
    expect(pane()).toHaveTextContent("3.33 s")
    expect(pane()).toHaveTextContent("48 kHz")
    expect(pane()).toHaveTextContent("Stereo")

    // Going back rechecks the file through the native loader; a deleted or changed file cannot reuse stale UI facts.
    press("Home")
    press("k")
    press("ArrowDown")
    expect(pane()).toHaveTextContent("Kick 01.wav")
    expect(asked).toHaveLength(3)
    await act(async () => asked[2].answer(infoFor("Kick 01.wav", 1.11)))
    expect(pane()).toHaveTextContent("1.11 s")
  })

  it("says so when a sound cannot be read", async () => {
    const { user } = await openKicks(() => ({
      sampleInfo: () => Promise.reject(new Error("Not a WAV file")),
    }))
    await user.click(item("Kick 01.wav"))
    expect(
      await screen.findByText("Could not read this sound. Not a WAV file")
    ).toBeInTheDocument()
    // It can still be added; the engine may know more than the overview.
    expect(screen.getByRole("button", { name: "Add to rack" })).toBeEnabled()
  })

  it("says it once for a broken file, with the file's name instead of its path", async () => {
    const broken = (path: string) =>
      Promise.reject(new Error(`Could not decode "${path}": bad header`))
    const { user } = await openKicks(() => ({
      sampleInfo: broken,
      previewPlay: broken,
    }))
    await user.click(item("Kick 01.wav"))
    expect(
      await screen.findByText(
        'Could not read this sound. Could not decode "Kick 01.wav": bad header'
      )
    ).toBeInTheDocument()
    await waitFor(() =>
      expect(useBrowserStore.getState().previewError).not.toBeNull()
    )
    expect(within(pane()).getAllByRole("alert")).toHaveLength(1)
    expect(pane()).not.toHaveTextContent("/factory")
  })
})

describe("factory loops", () => {
  const HOUSE = "/factory/Loops/House"
  const afterglow = {
    id: "house-a",
    name: "Afterglow",
    genre: "House",
    tempo: 124,
    key: 9,
    mode: "minor",
    bars: 2,
    provenance: "",
  }

  it("adds a factory loop at the song's tempo, as the Loops tab does", async () => {
    const user = userEvent.setup()
    const loopInsert = vi.fn<Backend["loopInsert"]>(() =>
      Promise.reject(new Error("not in this test"))
    )
    const app = await startBrowserTest((mock) => ({
      browserList: async (path) =>
        path === "/factory/Loops"
          ? [{ name: "House", path: HOUSE, kind: "folder" }]
          : path === HOUSE
            ? [{ name: "Afterglow.wav", path: `${HOUSE}/Afterglow.wav`, kind: "audio" }]
            : mock.browserList(path),
      loopCatalog: async () => ({ generation: 3, revision: 5, entries: [afterglow] }),
      loopInsert,
    }))
    stop = app.stop
    render(<BrowserPanel />)
    await user.click(await findItem("Loops"))
    await user.click(await findItem("House"))
    await user.click(await findItem("Afterglow.wav"))

    const add = await within(pane()).findByRole("button", { name: /^Add at .+ BPM$/ })
    expect(within(pane()).queryByRole("button", { name: "Add to playlist" })).toBeNull()
    await user.click(add)
    await waitFor(() =>
      expect(loopInsert).toHaveBeenCalledWith("house-a", null, null, expect.any(Number), 3, 5)
    )
  })

  it("keeps Add to playlist for a sound that is not a factory loop", async () => {
    await openKicks(() => ({
      loopCatalog: async () => ({ generation: 0, revision: 0, entries: [afterglow] }),
    }))
    fireEvent.mouseDown(item("Kick 01.wav"))
    expect(
      await within(pane()).findByRole("button", { name: "Add to playlist" })
    ).toBeInTheDocument()
    expect(within(pane()).queryByRole("button", { name: /^Add at/ })).toBeNull()
  })
})

/**
 * `openKicks` for tests that run on fake timers. user-event waits on real
 * timers, so the folders are opened with plain events instead.
 */
async function openKicksWithFakeTimers() {
  const app = await startBrowserTest()
  stop = app.stop
  render(<BrowserPanel />)
  const flush = () => act(async () => vi.advanceTimersByTimeAsync(1))
  await flush()
  fireEvent.click(item("Drums"))
  await flush()
  fireEvent.mouseDown(item("Kicks"))
  fireEvent.click(item("Kicks"))
  await flush()
  return { backend: app.backend }
}
