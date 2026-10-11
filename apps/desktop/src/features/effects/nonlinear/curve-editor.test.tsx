import { fireEvent, render, screen } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"
import { CurveEditor } from "./curve-editor"
import { XyPad } from "./performance-editors"

describe("drawn effect assets", () => {
  it("previews pointer movement locally and publishes once on release", () => {
    const commit = vi.fn()
    render(<CurveEditor label="Transfer" points={[]} onCommit={commit} />)
    const drawing = screen.getByRole("application", { name: "Transfer drawing" })
    drawing.getBoundingClientRect = () => ({ left: 0, top: 0, width: 100, height: 100, right: 100, bottom: 100, x: 0, y: 0, toJSON: () => ({}) })
    drawing.setPointerCapture = vi.fn()
    drawing.releasePointerCapture = vi.fn()
    fireEvent.pointerDown(drawing, { pointerId: 1, button: 0, clientX: 50, clientY: 60 })
    fireEvent.pointerMove(drawing, { pointerId: 1, clientX: 60, clientY: 30 })
    expect(commit).not.toHaveBeenCalled()
    fireEvent.pointerUp(drawing, { pointerId: 1 })
    expect(commit).toHaveBeenCalledTimes(1)
    expect(commit.mock.calls[0][0]).toContainEqual({ x: 0.6, y: 0.7, tension: 0 })
  })

  it("cancel and focus loss discard the draft, keyboard edits and bends commit", () => {
    const commit = vi.fn()
    render(<CurveEditor label="Time" points={[]} onCommit={commit} />)
    const drawing = screen.getByRole("application", { name: "Time drawing" })
    drawing.setPointerCapture = vi.fn()
    fireEvent.pointerDown(drawing, { pointerId: 1, button: 0, clientX: 1, clientY: 1 })
    fireEvent.keyDown(drawing, { key: "Escape" })
    fireEvent.pointerUp(drawing, { pointerId: 1 })
    expect(commit).not.toHaveBeenCalled()
    fireEvent.keyDown(drawing, { key: "ArrowUp" })
    expect(commit).toHaveBeenCalledTimes(1)
    fireEvent.change(screen.getByRole("spinbutton", { name: "Time tension" }), { target: { value: "0.5" } })
    expect(commit.mock.calls.at(-1)?.[0][0].tension).toBe(0.5)
  })
})

describe("performance pad", () => {
  it("steers independent axes, clamps and releases the gesture on capture loss", () => {
    const start=vi.fn(),change=vi.fn(),end=vi.fn()
    render(<XyPad x={0.5} y={0.5} onStart={start} onChange={change} onEnd={end} />)
    const pad=screen.getByRole("application",{name:"Performance XY pad"})
    pad.getBoundingClientRect=()=>({left:0,top:0,width:100,height:100,right:100,bottom:100,x:0,y:0,toJSON:()=>({})})
    pad.setPointerCapture=vi.fn();pad.hasPointerCapture=()=>true
    fireEvent.pointerDown(pad,{pointerId:1,button:0,clientX:20,clientY:70})
    expect(start).toHaveBeenCalledTimes(1);expect(change).toHaveBeenLastCalledWith(0.2,1-0.7)
    fireEvent.pointerMove(pad,{pointerId:1,clientX:200,clientY:-10})
    expect(change).toHaveBeenLastCalledWith(1,1)
    fireEvent.lostPointerCapture(pad,{pointerId:1})
    expect(end).toHaveBeenCalled()
  })
})
