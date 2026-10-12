import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { MockBackend } from "@/lib/ipc/mock"
import { settle, startTestApp } from "@/test/harness"

import { SpecialOutputEditor } from "./special-output-editor"

let backend: MockBackend
let stop: () => void
beforeEach(async()=>{({backend,stop}=await startTestApp())})
afterEach(()=>{stop();vi.restoreAllMocks()})
describe("system synth and controller feedback editor",()=>{
  it("shows unavailable routes without selecting an external device",async()=>{
    const state=await backend.midiHardwareState()
    const configure=vi.spyOn(backend,"midiHardwareConfigure")
    vi.spyOn(backend,"midiHardwareState").mockResolvedValue({...state,outputs:[]})
    render(<SpecialOutputEditor/>);await act(()=>settle())
    expect(screen.getByRole("button",{name:"Use system General MIDI synth"})).toBeDisabled()
    expect(screen.getByRole("button",{name:"Use game controller feedback"})).toBeDisabled()
    expect(configure).not.toHaveBeenCalled()
  })
  it("selects an advertised special route while preserving current settings",async()=>{
    const base=await backend.midiHardwareState()
    const state={...base,outputs:[{id:"windfall:system-general-midi",name:"System General MIDI synthesizer"},
      {id:"windfall:xinput-feedback",name:"Game controller feedback (XInput)"}]}
    vi.spyOn(backend,"midiHardwareState").mockResolvedValue(state)
    const configure=vi.spyOn(backend,"midiHardwareConfigure").mockImplementation(async settings=>({...state,settings,outputConnected:true}))
    render(<SpecialOutputEditor/>);await act(()=>settle())
    fireEvent.click(screen.getByRole("button",{name:"Use system General MIDI synth"}));await act(()=>settle())
    expect(configure).toHaveBeenLastCalledWith({...state.settings,output:"windfall:system-general-midi"})
    expect(screen.getByText("Selected output: System General MIDI synthesizer")).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button",{name:"Use game controller feedback"}));await act(()=>settle())
    expect(configure).toHaveBeenLastCalledWith({...state.settings,output:"windfall:xinput-feedback"})
    expect(screen.getByText("Selected output: Game controller feedback (XInput)")).toBeInTheDocument()
  })
  it("reports worker route failure",async()=>{
    const base=await backend.midiHardwareState()
    vi.spyOn(backend,"midiHardwareState").mockResolvedValue({...base,outputs:[{id:"windfall:xinput-feedback",name:"Controller"}]})
    vi.spyOn(backend,"midiHardwareConfigure").mockRejectedValue(new Error("Controller unplugged"))
    render(<SpecialOutputEditor/>);await act(()=>settle())
    fireEvent.click(screen.getByRole("button",{name:"Use game controller feedback"}));await act(()=>settle())
    expect(screen.getByRole("alert")).toHaveTextContent("Controller unplugged")
  })
})
