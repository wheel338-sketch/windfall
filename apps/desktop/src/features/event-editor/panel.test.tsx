import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { AutomationPoint, AutomationTarget, Pattern, Project } from "@/bindings"
import { automationEntries } from "@/features/automation/menu"
import { GAIN_RANGE, rangeNormalized } from "@/lib/automation/curve"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { useTransportStore } from "@/lib/store/transport"

import {
  closeEventEditor,
  openEventEditor,
  registerEventEditorActions,
  useEventEditor,
} from "./actions"
import { EventEditorPanel } from "./panel"

/*
 * The event editor window over a project held in the store. The document
 * itself is covered natively (`crates/windfall-project/tests/pattern_events_qa.rs`);
 * here every gesture is checked for the one command it sends.
 */

vi.mock("@/lib/store/project", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/store/project")>()),
  dispatch: vi.fn(),
}))
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
  Toaster: () => null,
}))

const VOLUME: AutomationTarget = { type: "channelVolume", channel: 5 }
const PAN: AutomationTarget = { type: "channelPan", channel: 5 }
const point = (tick: number, value: number): AutomationPoint => ({
  tick,
  value,
  curve: 0,
  hold: false,
})

const original = useProjectStore.getState()
const transport = useTransportStore.getState()

function load(events: Pattern["events"] = undefined) {
  const project = {
    ...original.project,
    channels: [{ id: 5, name: "Lead", volume: 0.8, pan: 0 }],
    mixer: {
      tracks: [{ id: 0, name: "Master", volume: 1, pan: 0, effects: [], sends: [] }],
    },
    patterns: [
      { id: 1, name: "Verse", color: 0, lengthSteps: 16, lanes: [], events },
      { id: 2, name: "Chorus", color: 0, lengthSteps: 32, lanes: [] },
    ],
    automations: [],
  } as unknown as Project
  useProjectStore.setState({ project })
  useTransportStore.setState({ pattern: 1 })
}

const sent = () => vi.mocked(dispatch).mock.calls
const curve = () => screen.getByRole("application", { name: "Event curve" })
const handles = () => screen.queryAllByRole("slider")

beforeEach(() => {
  vi.mocked(dispatch).mockResolvedValue({} as never)
  // jsdom lays nothing out: give the curve the size of its own view box.
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    left: 0,
    top: 0,
    right: 720,
    bottom: 220,
    width: 720,
    height: 220,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  })
})

afterEach(() => {
  act(() => {
    useEventEditor.setState({ open: false, pattern: null, target: null })
  })
  useProjectStore.setState(original)
  useTransportStore.setState(transport)
  vi.restoreAllMocks()
  vi.mocked(dispatch).mockReset()
})

describe("event editor", () => {
  it("opens on the selected pattern and waits for a control without editing", async () => {
    load()
    render(<EventEditorPanel />)
    expect(screen.queryByText("Event editor")).toBeNull()
    act(() => openEventEditor())
    expect(await screen.findByText("Event editor")).toBeInTheDocument()
    expect(
      screen.getByRole("combobox", { name: "Event pattern" })
    ).toHaveTextContent("Verse")
    expect(screen.queryByRole("application", { name: "Event curve" })).toBeNull()
    expect(screen.getByText(/Choose a control/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Close" }))
    expect(useEventEditor.getState().open).toBe(false)
    expect(sent()).toHaveLength(0)
  })

  it("adds the first point at the value the control has now", async () => {
    load()
    render(<EventEditorPanel />)
    act(() => openEventEditor(VOLUME))
    await screen.findByRole("application", { name: "Event curve" })
    expect(handles()).toHaveLength(0)
    expect(screen.getByText(/has no events for this control/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole("button", { name: "Add point at start" }))
    await waitFor(() => expect(sent()).toHaveLength(1))
    const [command, gesture] = sent()[0]
    expect(command).toEqual({
      type: "setPatternEvents",
      pattern: 1,
      target: VOLUME,
      expected: [],
      points: [point(0, rangeNormalized(GAIN_RANGE, 0.8))],
    })
    expect(gesture).toBeUndefined()
    // The draft shows at once.
    expect(handles()).toHaveLength(1)
  })

  it("adds a point where the curve is double-clicked", async () => {
    load([{ id: 40, target: VOLUME, points: [point(0, 0.2), point(3840, 0.8)] }])
    render(<EventEditorPanel />)
    act(() => openEventEditor(VOLUME))
    await screen.findByRole("application", { name: "Event curve" })
    expect(handles()).toHaveLength(2)
    // The middle of the drawing area is half way through the pattern at 50%.
    fireEvent.doubleClick(curve(), { clientX: 360, clientY: 110 })
    await waitFor(() => expect(sent()).toHaveLength(1))
    expect(sent()[0][0]).toEqual({
      type: "setPatternEvents",
      pattern: 1,
      target: VOLUME,
      expected: [point(0, 0.2), point(3840, 0.8)],
      points: [point(0, 0.2), point(1920, 0.5), point(3840, 0.8)],
    })
    expect(handles()).toHaveLength(3)
  })

  it("moves a point with the keys as one gesture and removes it with Delete", async () => {
    const lane = [point(0, 0.2), point(960, 0.5), point(3840, 0.8)]
    load([{ id: 40, target: VOLUME, points: lane }])
    render(<EventEditorPanel />)
    act(() => openEventEditor(VOLUME))
    await screen.findByRole("application", { name: "Event curve" })
    const middle = handles()[1]
    fireEvent.focus(middle)
    fireEvent.keyDown(middle, { key: "ArrowUp" })
    fireEvent.keyDown(middle, { key: "ArrowRight" })
    await waitFor(() => expect(sent()).toHaveLength(2))
    const [first, firstGesture] = sent()[0]
    const [second, secondGesture] = sent()[1]
    expect(first).toMatchObject({ type: "setPatternEvents", expected: lane })
    expect((first as { points: AutomationPoint[] }).points[1].value).toBeCloseTo(0.51)
    expect((first as { points: AutomationPoint[] }).points[1].tick).toBe(960)
    // The second step builds on the first and belongs to the same undo step.
    expect((second as { expected: AutomationPoint[] }).expected).toEqual(
      (first as { points: AutomationPoint[] }).points
    )
    // A key press moves by a sixteenth: a quarter of the beat.
    expect((second as { points: AutomationPoint[] }).points[1].tick).toBe(960 + 240)
    expect(typeof firstGesture).toBe("number")
    expect(secondGesture).toBe(firstGesture)
    fireEvent.keyUp(middle, { key: "ArrowRight" })

    fireEvent.keyDown(handles()[1], { key: "Delete" })
    await waitFor(() => expect(sent()).toHaveLength(3))
    const [third, thirdGesture] = sent()[2]
    expect((third as { points: AutomationPoint[] }).points).toEqual([
      point(0, 0.2),
      point(3840, 0.8),
    ])
    expect(thirdGesture).toBeUndefined()
  })

  it("edits the selected point's fields and clears the lane", async () => {
    const lane = [point(0, 0.2), point(960, 0.5)]
    load([{ id: 40, target: PAN, points: lane }])
    render(<EventEditorPanel />)
    act(() => openEventEditor(PAN))
    await screen.findByRole("application", { name: "Event curve" })
    expect(screen.getByLabelText("Value (%)")).toBeDisabled()
    fireEvent.focus(handles()[1])
    expect(screen.getByLabelText("Tick")).toHaveValue(960)
    expect(screen.getByLabelText("Value (%)")).toHaveValue(50)

    fireEvent.change(screen.getByLabelText("Value (%)"), { target: { value: "25" } })
    await waitFor(() => expect(sent()).toHaveLength(1))
    expect((sent()[0][0] as { points: AutomationPoint[] }).points[1]).toEqual(
      point(960, 0.25)
    )
    fireEvent.change(screen.getByLabelText("Bend (%)"), { target: { value: "-40" } })
    await waitFor(() => expect(sent()).toHaveLength(2))
    expect((sent()[1][0] as { points: AutomationPoint[] }).points[1].curve).toBeCloseTo(-0.4)
    fireEvent.click(screen.getByRole("switch", { name: "Hold until next" }))
    await waitFor(() => expect(sent()).toHaveLength(3))
    expect((sent()[2][0] as { points: AutomationPoint[] }).points[1].hold).toBe(true)

    fireEvent.click(screen.getByRole("button", { name: "Clear lane" }))
    await waitFor(() => expect(sent()).toHaveLength(4))
    expect(sent()[3][0]).toMatchObject({
      type: "setPatternEvents",
      pattern: 1,
      target: PAN,
      points: [],
    })
  })

  it("says so and shows the project's curve when an edit is refused", async () => {
    load([{ id: 40, target: VOLUME, points: [point(0, 0.2)] }])
    vi.mocked(dispatch).mockResolvedValue(null)
    render(<EventEditorPanel />)
    act(() => openEventEditor(VOLUME))
    await screen.findByRole("application", { name: "Event curve" })
    fireEvent.doubleClick(curve(), { clientX: 360, clientY: 110 })
    expect(
      await screen.findByText(/changed while it was being edited/)
    ).toBeInTheDocument()
    await waitFor(() => expect(handles()).toHaveLength(1))
  })

  it("follows the pattern it is given and a project that changes under it", async () => {
    load([{ id: 40, target: VOLUME, points: [point(0, 0.2)] }])
    render(<EventEditorPanel />)
    act(() => openEventEditor(VOLUME, 2))
    await screen.findByRole("application", { name: "Event curve" })
    expect(
      screen.getByRole("combobox", { name: "Event pattern" })
    ).toHaveTextContent("Chorus")
    // The other pattern has no lane for the control.
    expect(handles()).toHaveLength(0)
    act(() => useEventEditor.setState({ pattern: 1 }))
    await waitFor(() => expect(handles()).toHaveLength(1))
    // An undo elsewhere takes the lane away: the editor shows that.
    act(() => load())
    await waitFor(() => expect(handles()).toHaveLength(0))
    expect(sent()).toHaveLength(0)
  })

  it("is reached from a control's menu and from the action registry", () => {
    load()
    const entry = automationEntries(VOLUME).find(
      (item) =>
        typeof item === "object" &&
        "title" in item &&
        item.title === "Edit events in pattern"
    )
    expect(entry).toBeDefined()
    ;(entry as { run(): void }).run()
    expect(useEventEditor.getState()).toMatchObject({ open: true, target: VOLUME })
    closeEventEditor()

    // The tempo belongs to the song and has no such entry.
    expect(
      automationEntries({ type: "tempo" }).some(
        (item) =>
          typeof item === "object" &&
          "title" in item &&
          item.title === "Edit events in pattern"
      )
    ).toBe(false)
    openEventEditor({ type: "tempo" })
    expect(useEventEditor.getState().target).toEqual(VOLUME)

    const stop = registerEventEditorActions()
    stop()
    expect(useEventEditor.getState().open).toBe(false)
  })
})
