import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { RealtimeFrame } from "@/bindings"
import { useTransportStore } from "@/lib/store/transport"
import { NotebookPanel } from "./panel"
import { pageAtTick } from "./playback"

const feed = vi.hoisted(() => ({ listener: null as null | ((frame: RealtimeFrame) => void), stop: vi.fn() }))
vi.mock("@/lib/store/realtime", () => ({ subscribeRealtime: (listener: (frame: RealtimeFrame) => void) => {
  feed.listener = listener
  return feed.stop
} }))
afterEach(() => { feed.listener = null; vi.clearAllMocks(); useTransportStore.setState({ mode: "pattern" }) })
const pages = [
  { title: "Verse", body: "First", startTick: 0 },
  { title: "Chorus", body: "Second", startTick: 3840 },
  { title: "Manual", body: "Last" },
]
describe("notebook playback cues", () => {
  it("selects latest cue on boundaries, seeks and loops without depending on page order", () => {
    expect(pageAtTick(pages, 3839)).toBe(0)
    expect(pageAtTick(pages, 3840)).toBe(1)
    expect(pageAtTick(pages, 0)).toBe(0)
    expect(pageAtTick([pages[1], pages[0]], 4000)).toBe(0)
    expect(pageAtTick([pages[0], pages[0]], 1)).toBe(0)
    expect(pageAtTick([pages[2]], 4000)).toBeNull()
    expect(pageAtTick(pages, NaN)).toBeNull()
  })
  it("mounted follow selects pages only during song playback and releases its feed", () => {
    const save = vi.fn(async () => true)
    const { unmount } = render(<NotebookPanel notebook={{ pages }} onSave={save} />)
    fireEvent.click(screen.getByRole("checkbox", { name: "Follow song playback" }))
    const send = (tick: number, playing = true) => act(() => feed.listener?.({ tick, playing } as RealtimeFrame))
    send(4000)
    expect(screen.getByLabelText("Page title")).toHaveValue("Verse")
    useTransportStore.setState({ mode: "song" })
    send(4000)
    expect(screen.getByLabelText("Page title")).toHaveValue("Chorus")
    send(0, false)
    expect(screen.getByLabelText("Page title")).toHaveValue("Chorus")
    send(0)
    expect(screen.getByLabelText("Page title")).toHaveValue("Verse")
    expect(save).not.toHaveBeenCalled()
    unmount()
    expect(feed.stop).toHaveBeenCalled()
  })
  it("saves cue and HTML mode in the existing whole-book transaction", async () => {
    const save = vi.fn(async () => true)
    render(<NotebookPanel notebook={{ pages: [pages[0]] }} onSave={save} />)
    fireEvent.change(screen.getByLabelText("Page starts at song tick"), { target: { value: "960" } })
    fireEvent.click(screen.getByRole("checkbox", { name: "Safe HTML preview" }))
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Save notebook" })))
    expect(save).toHaveBeenCalledWith({ pages: [{ ...pages[0], startTick: 960, html: true }] })
  })
})
