import { fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { CurveEditor } from "./curve-editor"
import { defaultCurve } from "./settings"

afterEach(() => vi.restoreAllMocks())
function mount() {
  const changed = vi.fn()
  render(<CurveEditor curve={defaultCurve()} onChange={changed} />)
  const surface = screen.getByRole("img", { name: "Draw playback curve" })
  vi.spyOn(surface, "getBoundingClientRect").mockReturnValue({
    x: 0, y: 0, left: 0, top: 0, right: 300, bottom: 100,
    width: 300, height: 100, toJSON: () => ({}),
  })
  return { surface, changed }
}
describe("drawn playback curve gestures", () => {
  it("commits a multi-move drawing gesture once with ordered source positions", () => {
    const { surface, changed } = mount()
    fireEvent.pointerDown(surface, { pointerId: 1, button: 0, clientX: 150, clientY: 50 })
    fireEvent.pointerMove(surface, { pointerId: 1, clientX: 180, clientY: 10 })
    fireEvent.pointerMove(surface, { pointerId: 1, clientX: 210, clientY: 20 })
    expect(changed).not.toHaveBeenCalled()
    fireEvent.pointerUp(surface, { pointerId: 1 })
    expect(changed).toHaveBeenCalledTimes(1)
    expect(changed.mock.calls[0][0].points).toEqual([
      { time: 0, value: 0 }, { time: 0.7, value: 0.8 }, { time: 1, value: 1 },
    ])
  })
  it("cancels draft drawing without publishing an asset or history edit", () => {
    const { surface, changed } = mount()
    fireEvent.pointerDown(surface, { pointerId: 1, button: 0, clientX: 150, clientY: 20 })
    fireEvent.pointerCancel(surface, { pointerId: 1 })
    expect(changed).not.toHaveBeenCalled()
    expect(screen.queryByRole("spinbutton", { name: "Point 3 value" })).toBeNull()
  })
})
