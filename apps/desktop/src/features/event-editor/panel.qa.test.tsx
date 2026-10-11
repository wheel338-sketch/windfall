import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { announceProjectReplaced } from "@/lib/store/replaced"
import { automatedNow } from "@/features/automation/now"
import { settle } from "@/test/harness"
import { channel, currentPattern, startRoll, undoSteps } from "@/features/piano-roll/test-utils"
import { openEventEditor, closeEventEditor } from "./actions"
import { EventEditorPanel } from "./panel"

let roll: Awaited<ReturnType<typeof startRoll>>
beforeEach(async () => {
  roll = await startRoll()
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({ x: 0, y: 0, left: 0, top: 0, right: 720, bottom: 220, width: 720, height: 220, toJSON: () => ({}) })
  render(<EventEditorPanel />)
  act(() => openEventEditor({ type: "channelVolume", channel: channel("Kick").id }, currentPattern().id))
})
afterEach(() => { cleanup(); closeEventEditor(); roll.stop(); vi.restoreAllMocks() })

it("mounted native event edits snap, survive one-step undo/redo/save and report playback values", async () => {
  const user = userEvent.setup()
  const before = undoSteps()
  await user.click(screen.getByRole("combobox", { name: "Event snap" }))
  await user.click(screen.getByRole("option", { name: "Step", exact: true }))
  await act(async () => { fireEvent.doubleClick(screen.getByRole("application", { name: "Event curve" }), { clientX: 100, clientY: 60 }); await settle() })
  const events = structuredClone(currentPattern().events!)
  expect(events[0].points[0].tick).toBe(480)
  expect(undoSteps()).toBe(before + 1)
  await roll.backend.projectSave("/projects/events-qa.windfall")
  await act(async () => { await undo(); await settle() })
  expect(currentPattern().events ?? []).toEqual([])
  await act(async () => { await redo(); await settle() })
  expect(currentPattern().events).toEqual(events)
  await act(async () => { await undo(); await roll.backend.projectOpen("/projects/events-qa.windfall"); await settle() })
  expect(currentPattern().events).toEqual(events)
  act(() => openEventEditor(events[0].target, currentPattern().id))
  await act(async () => { await roll.backend.transportSet({ mode: "pattern", pattern: currentPattern().id }); await roll.backend.transportPlay(); await settle() })
  await waitFor(() => expect(automatedNow(events[0].target)?.value).toBeDefined())
  expect(automatedNow(events[0].target)?.color).toBeDefined()
  await waitFor(() => expect(screen.getByLabelText("Event playhead").style.display).toBe(""))
  await roll.backend.transportStop()
})

it("refuses raced lane edits and queued writes from a replaced project", async () => {
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add point at start" })); await settle() })
  const real = roll.backend.dispatch.bind(roll.backend)
  vi.spyOn(roll.backend, "dispatch").mockImplementationOnce(async (command, gesture) => {
    await real({ type: "setPatternEvents", pattern: currentPattern().id, target: currentPattern().events![0].target,
      expected: currentPattern().events![0].points, points: [{ tick: 0, value: .9, curve: 0, hold: false }] })
    return real(command, gesture)
  })
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Clear lane" })); await settle() })
  expect(currentPattern().events![0].points[0].value).toBeCloseTo(.9)
  expect(screen.getByText(/lane changed while/i)).toBeVisible()
  const after = useProjectStore.getState()
  act(() => { fireEvent.click(screen.getByRole("button", { name: "Clear lane" })); announceProjectReplaced() })
  await act(settle)
  expect(useProjectStore.getState()).toBe(after)
  expect(screen.queryByRole("dialog")).toBeNull()
})
