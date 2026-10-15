import { act, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { rowMenu } from "@/features/browser/row-menu"
import type { EntryRow } from "@/features/browser/tree-model"
import type { MockBackend } from "@/lib/ipc/mock"
import { dispatch, useProjectStore } from "@/lib/store"
import { useUiStore } from "@/lib/store/ui"
import { settle, startTestApp } from "@/test/harness"

import {
  addChannelFromSoundFont,
  addGeneralMidiChannel,
  GeneralMidiSoundFontSection,
  SoundFontChannelDialogHost,
  useSoundFontChannelDialog,
} from "./soundfont-channel"

let backend: MockBackend
let stop: () => void
beforeEach(async () => { ({ backend, stop } = await startTestApp()) })
afterEach(() => { useSoundFontChannelDialog.getState().close(); stop(); vi.restoreAllMocks() })

const PRESETS = [
  { name: "Grand", bank: 0, program: 0 },
  { name: "Horn", bank: 0, program: 60 },
  { name: "Choir", bank: 0, program: 52 },
  { name: "Standard", bank: 128, program: 0 },
]

/** The backend adds a SoundFont channel as the session does: one dispatch, one undo step. */
function stubAdd() {
  return vi.fn(async (_path: string, bank: number, program: number) =>
    backend.dispatch({ type: "batch", label: "Add SoundFont channel", commands: [
      { type: "addChannel", name: PRESETS.find((p) => p.bank === bank && p.program === program)?.name, instrument: "soundFont" },
    ] }))
}

describe("adding a channel from a SoundFont", () => {
  it("lists a browser SoundFont's presets by family and adds the chosen one as one channel", async () => {
    backend.soundfontPresets = vi.fn().mockResolvedValue(PRESETS)
    const add = stubAdd()
    backend.soundfontAddChannel = add
    render(<SoundFontChannelDialogHost />)
    act(() => addChannelFromSoundFont("C:/Sounds/GM.sf2"))
    await act(() => settle())
    const dialog = within(screen.getByRole("dialog", { name: "Add a channel from a SoundFont" }))
    expect(dialog.getByText("Choose a preset of GM.sf2. The new channel plays it, as one undo step.")).toBeInTheDocument()
    expect(backend.soundfontPresets).toHaveBeenCalledWith("C:/Sounds/GM.sf2")
    const families = within(dialog.getByRole("group", { name: "SoundFont presets" })).getAllByRole("group").map((group) => group.getAttribute("aria-label"))
    expect(families).toEqual(["Piano", "Ensemble", "Brass", "Drum Kits"])
    const before = useProjectStore.getState().project.channels.length
    fireEvent.click(dialog.getByRole("button", { name: "0:60 · Horn" }))
    await act(() => settle())
    expect(add).toHaveBeenCalledWith("C:/Sounds/GM.sf2", 0, 60, null)
    expect(useProjectStore.getState().project.channels).toHaveLength(before + 1)
    expect(useProjectStore.getState().project.channels.at(-1)?.name).toBe("Horn")
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
  })

  it("offers General MIDI instruments from the SoundFont chosen in Settings, and says when there is none", async () => {
    backend.soundfontPresets = vi.fn().mockResolvedValue(PRESETS)
    backend.soundfontAddChannel = stubAdd()
    backend.defaultSoundfont = vi.fn().mockResolvedValue(null)
    render(<SoundFontChannelDialogHost />)
    await act(() => addGeneralMidiChannel())
    await act(() => settle())
    let dialog = within(screen.getByRole("dialog", { name: "Add a General MIDI instrument" }))
    expect(dialog.getByText("Choose a General MIDI SoundFont in Settings first. Windfall does not include one.")).toBeInTheDocument()
    fireEvent.click(dialog.getByRole("button", { name: "Open Settings" }))
    expect(useUiStore.getState().dialog).toBe("settings")
    expect(screen.queryByRole("dialog", { name: "Add a General MIDI instrument" })).not.toBeInTheDocument()
    backend.defaultSoundfont = vi.fn().mockResolvedValue("C:/Banks/gm.sf2")
    await act(() => addGeneralMidiChannel())
    await act(() => settle())
    dialog = within(screen.getByRole("dialog", { name: "Add a General MIDI instrument" }))
    fireEvent.change(dialog.getByRole("searchbox", { name: "Search presets" }), { target: { value: "ensemble" } })
    expect(dialog.getAllByRole("button", { name: /^0:/ }).map((button) => button.textContent)).toEqual(["0:52 · Choir"])
    fireEvent.click(dialog.getByRole("button", { name: "0:52 · Choir" }))
    await act(() => settle())
    expect(backend.soundfontAddChannel).toHaveBeenCalledWith("C:/Banks/gm.sf2", 0, 52, null)
  })

  it("shows a SoundFont that cannot be read instead of presets", async () => {
    backend.soundfontPresets = vi.fn().mockRejectedValue(new Error("Unsupported SoundFont content"))
    backend.soundfontAddChannel = stubAdd()
    render(<SoundFontChannelDialogHost />)
    act(() => addChannelFromSoundFont("C:/Sounds/broken.sf2"))
    await act(() => settle())
    expect(screen.getByRole("alert")).toHaveTextContent("Unsupported SoundFont content")
    expect(screen.queryByRole("group", { name: "SoundFont presets" })).not.toBeInTheDocument()
  })
})

describe("the General MIDI SoundFont setting", () => {
  it("is chosen with the SoundFont picker and can be forgotten", async () => {
    let kept: string | null = null
    backend.defaultSoundfont = vi.fn(async () => kept)
    backend.setDefaultSoundfont = vi.fn(async (path: string | null) => { kept = path; return path })
    const picked = vi.spyOn(backend, "pickAudioFile").mockResolvedValue("C:/Banks/gm.sf2")
    render(<GeneralMidiSoundFontSection />)
    await act(() => settle())
    const section = within(screen.getByRole("region", { name: "General MIDI SoundFont" }))
    expect(section.getByText("None chosen")).toBeInTheDocument()
    fireEvent.click(section.getByRole("button", { name: "Choose file…" }))
    await act(() => settle())
    expect(picked).toHaveBeenCalledWith("soundFont")
    expect(backend.setDefaultSoundfont).toHaveBeenCalledWith("C:/Banks/gm.sf2")
    expect(section.getByText("C:/Banks/gm.sf2")).toBeInTheDocument()
    fireEvent.click(section.getByRole("button", { name: "Forget it" }))
    await act(() => settle())
    expect(backend.setDefaultSoundfont).toHaveBeenLastCalledWith(null)
    expect(section.getByText("None chosen")).toBeInTheDocument()
  })

  it("shows why a file was refused", async () => {
    backend.defaultSoundfont = vi.fn(async () => null)
    backend.setDefaultSoundfont = vi.fn().mockRejectedValue(new Error("C:/x.wav is not a SoundFont (.sf2) file"))
    vi.spyOn(backend, "pickAudioFile").mockResolvedValue("C:/x.wav")
    render(<GeneralMidiSoundFontSection />)
    await act(() => settle())
    fireEvent.click(screen.getByRole("button", { name: "Choose file…" }))
    await act(() => settle())
    expect(screen.getByRole("alert")).toHaveTextContent("is not a SoundFont")
  })
})

describe("SoundFonts in the browser", () => {
  it("are rows of their own whose menu adds a channel from them", async () => {
    await dispatch({ type: "addChannel" })
    const row = { type: "entry", kind: "soundFont", path: "C:/Sounds/GM.sf2", name: "GM.sf2" } as unknown as EntryRow
    const items = rowMenu(row, null)
    const add = items.find((item) => typeof item === "object" && "title" in item && item.title === "Add a channel from this SoundFont…")
    expect(add).toBeDefined()
    if (typeof add !== "object" || !("run" in add) || !add.run) throw new Error("no run")
    act(() => { void add.run?.() })
    expect(useSoundFontChannelDialog.getState().request).toEqual({ path: "C:/Sounds/GM.sf2", generalMidi: false })
  })
})
