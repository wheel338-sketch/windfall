import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { startRoll, currentPattern, channel, notesOf } from "@/features/piano-roll/test-utils"
import { ScriptEditor } from "./script-editor"
import { closeScripts, openScripts } from "./store"
import userEvent from "@testing-library/user-event"
import { SimDocument } from "@/lib/ipc/sim/document"

let roll: Awaited<ReturnType<typeof startRoll>>
beforeEach(async () => { roll = await startRoll() })
afterEach(() => { cleanup(); closeScripts(); roll.stop(); vi.restoreAllMocks() })
function open() { render(<TooltipProvider><ScriptEditor /></TooltipProvider>); act(() => openScripts()) }

describe("script editor mounted cases (worker responses are explicit fixtures)", () => {
  it("reports native worker capability and builds parameter controls with the Rust parser", async () => {
    open()
    expect(screen.getByRole("button", { name: "Preview notes" })).toBeDisabled()
    expect(screen.getByText(/Piano execution and hardware controller scripts require/)).toBeVisible()
    fireEvent.click(screen.getByRole("button", { name: "Read parameters" }))
    expect(await screen.findByLabelText("transpose (-24..24)")).toHaveValue(12)
    fireEvent.change(screen.getByLabelText("Windfall Script v1 source"), { target: { value: "set key +" } })
    fireEvent.click(screen.getByRole("button", { name: "Read parameters" }))
    expect(await screen.findByText(/Line 1: expression stack underflow/)).toBeVisible()
  })
  it("reviews saved revisions, applies through Rust, reopens in the editor and undoes", async () => {
    open()
    const before = useProjectStore.getState().history.cursor
    fireEvent.click(screen.getByRole("button", { name: "Review save revision" }))
    expect(await screen.findByText("setScriptLibrary")).toBeVisible()
    expect(useProjectStore.getState().project.settings.scripts).toBeUndefined()
    fireEvent.click(screen.getByRole("button", { name: "Apply batch" }))
    await waitFor(() => expect(useProjectStore.getState().project.settings.scripts?.assets).toHaveLength(1))
    expect(useProjectStore.getState().history.cursor).toBe(before + 1)
    expect(screen.getByRole("button", { name: "user-script r1" })).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Undo batch" }))
    await waitFor(() => expect(useProjectStore.getState().project.settings.scripts).toBeUndefined())
  })
  it("assigns a compiled transform to the selected channel with an exact saved revision", async () => {
    open()
    fireEvent.click(screen.getByRole("button", { name: "Note transform" }))
    fireEvent.click(screen.getByRole("button", { name: "Review save and assign" }))
    await screen.findByText("setScriptLibrary")
    fireEvent.click(screen.getByRole("button", { name: "Apply batch" }))
    await waitFor(() => expect(useProjectStore.getState().project.settings.scripts?.channels).toHaveLength(1))
    const binding = useProjectStore.getState().project.settings.scripts!.channels[0]
    expect(binding.channel).toBe(channel("Kick").id)
    expect(binding.script).toEqual({ id: "user-script", revision: 1 })
    fireEvent.click(screen.getByText("Installed assignments"))
    fireEvent.click(screen.getByRole("button", { name: `Review remove channel ${binding.channel}` }))
    await screen.findByText(/Remove this installed assignment/)
    fireEvent.click(screen.getByRole("button", { name: "Apply batch" }))
    await waitFor(() => expect(useProjectStore.getState().project.settings.scripts?.channels).toHaveLength(0))
    expect(useProjectStore.getState().project.settings.scripts?.assets).toHaveLength(1)
    fireEvent.click(screen.getByRole("button", { name: "Undo batch" }))
    await waitFor(() => expect(useProjectStore.getState().project.settings.scripts?.channels).toEqual([binding]))
  })
  it("captures selected IDs for a native preview, then applies the reviewed fixture atomically", async () => {
    Object.defineProperty(roll.backend, "kind", { value: "tauri", configurable: true })
    const added = await dispatch({ type: "addNotes", pattern: currentPattern().id, channel: channel("Kick").id, notes: [{ start: 1920, length: 240, key: 60 }] })
    roll.editor.setSelection(added!.created)
    vi.spyOn(roll.backend, "scriptBegin").mockResolvedValue(9)
    const preview = vi.spyOn(roll.backend, "scriptPreview").mockImplementation(async (_request, input) => ({
      parameters: [], notes: [{ start: 1920, length: 240, key: 72, velocity: 0.8, pan: 0 }],
      proposal: { stamp: input.stamp, label: "Piano script", explanation: "Fixture: replace selected note", commands: [
        { type: "removeNotes", pattern: input.pattern, channel: input.channel, notes: input.selected },
        { type: "addNotes", pattern: input.pattern, channel: input.channel, notes: [{ start: 1920, length: 240, key: 72 }] },
      ] },
    }))
    open()
    fireEvent.click(screen.getByRole("button", { name: "Preview notes" }))
    expect(await screen.findByText("1 emitted notes")).toBeVisible()
    expect(preview.mock.calls[0][1].selected).toEqual(added!.created)
    const cursor = useProjectStore.getState().history.cursor
    fireEvent.click(screen.getByRole("button", { name: "Apply batch" }))
    await waitFor(() => expect(notesOf("Kick").some((note) => note.start === 1920 && note.key === 72)).toBe(true))
    expect(notesOf("Kick").some((note) => added!.created.includes(note.id))).toBe(false)
    expect(useProjectStore.getState().history.cursor).toBe(cursor + 1)
  })
  it("requires reviewed controller installation and stores feedback opt-in with its exact source", async () => {
    Object.defineProperty(roll.backend, "kind", { value: "tauri", configurable: true })
    const preview = vi.spyOn(roll.backend, "scriptPreview")
    open()
    fireEvent.click(screen.getByRole("button", { name: "Controller" }))
    fireEvent.click(screen.getByRole("checkbox", { name: "Enable MIDI feedback" }))
    fireEvent.change(screen.getByLabelText("Input CC on MIDI channel 1"), { target: { value: "11" } })
    fireEvent.click(screen.getByRole("button", { name: "Review save and assign" }))
    await screen.findByText("setScriptLibrary")
    expect(useProjectStore.getState().project.settings.scripts).toBeUndefined()
    expect(preview).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole("button", { name: "Apply batch" }))
    await waitFor(() => expect(useProjectStore.getState().project.settings.scripts?.controllers).toHaveLength(1))
    const binding = useProjectStore.getState().project.settings.scripts!.controllers[0]
    expect(binding.source).toEqual({ type: "midi", channel: 1, control: { type: "cc", number: 11 } })
    expect(binding.feedbackChannel).toBe(1)
    expect(binding.target).toEqual({ type: "slot", slot: { type: "masterVolume" } })
    expect(binding.script).toEqual({ id: "user-script", revision: 1 })
    fireEvent.click(screen.getByText("Installed assignments"))
    fireEvent.click(screen.getByRole("button", { name: "Review remove controller 1" }))
    await screen.findByText(/Remove this installed assignment/)
    fireEvent.click(screen.getByRole("button", { name: "Apply batch" }))
    await waitFor(() => expect(useProjectStore.getState().project.settings.scripts?.controllers).toHaveLength(0))
  })
  it("reviews a named pressure binding with soft takeover, smoothing and a checked channel target and reopens it", async () => {
    Object.defineProperty(roll.backend, "kind", { value: "tauri", configurable: true })
    const user = userEvent.setup()
    open()
    await user.click(screen.getByRole("button", { name: "Controller" }))
    fireEvent.change(screen.getByLabelText("MIDI device name (blank means any enabled input)"), { target: { value: "Recorded surface" } })
    fireEvent.change(screen.getByLabelText("Input MIDI channel (0 means all)"), { target: { value: "3" } })
    await user.click(screen.getByRole("combobox", { name: "Script input type" }))
    await user.click(await screen.findByRole("option", { name: "Channel pressure" }))
    await user.click(screen.getByRole("combobox", { name: "Script mapped target" }))
    await user.click(await screen.findByRole("option", { name: "Kick → pan" }))
    await user.click(screen.getByRole("combobox", { name: "Script pickup" }))
    await user.click(await screen.findByRole("option", { name: "Soft takeover" }))
    fireEvent.change(screen.getByLabelText("Script smoothing (ms)"), { target: { value: "250" } })
    await user.click(screen.getByRole("button", { name: "Review save and assign" }))
    await screen.findByText("setScriptLibrary")
    await user.click(screen.getByRole("button", { name: "Apply batch" }))
    await waitFor(() => expect(useProjectStore.getState().project.settings.scripts?.controllers).toHaveLength(1))
    const binding = useProjectStore.getState().project.settings.scripts!.controllers[0]
    expect(binding.source).toEqual({ type: "midi", device: "Recorded surface", channel: 3, control: { type: "channelPressure" } })
    expect(binding.map.pickup).toBe("on")
    expect(binding.map.smoothingMs).toBe(250)
    expect(binding.target).toEqual({ type: "param", target: { type: "channelPan", channel: channel("Kick").id } })
    const saved = SimDocument.create(structuredClone(useProjectStore.getState().project))
    const reopened = SimDocument.open(saved.fileText())
    expect(reopened.project().settings.scripts?.controllers).toEqual([binding])
    saved.dispose(); reopened.dispose()
  })
})
