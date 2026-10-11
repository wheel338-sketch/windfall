import { openEffect } from "@/features/mixer/effect-ops"
import { toast } from "sonner"
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { startTestApp, settle } from "@/test/harness"
import { dispatch, undo, redo, useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { instrumentDescriptor } from "@/features/params/descriptors"
import { ProjectTab } from "@/features/browser/project-tab"
import { ProjectOverviewView } from "@/features/project-overview/view"
import { PluginPicker, usePluginPicker } from "@/features/plugins/picker"
import { PluginCatalog } from "@/features/plugins/catalog"
import { MidiDialogs } from "@/features/midi/dialogs"
import { PROJECT_TIMEBASES, ProjectPlaybackSettings } from "@/features/project-info/settings"
import { ProjectFilesPanel } from "./panel"
import { ScoreFileControls } from "./score-controls"
import { usePlaylistStore } from "@/features/playlist/store"
import { useTransportStore } from "@/lib/store/transport"

let app: Awaited<ReturnType<typeof startTestApp>>
let midiFiles: Uint8Array[] = []
beforeEach(async () => { midiFiles = []; app = await startTestApp({ midiExport: (_path, bytes) => { midiFiles.push(bytes) } }); useUiStore.getState().selectChannel(useProjectStore.getState().project.channels[0].id) })
afterEach(() => { usePluginPicker.setState({ open: false }); localStorage.removeItem("windfall.plugin-tile-images.v1"); app.stop(); vi.restoreAllMocks() })
const state = () => useProjectStore.getState()

it("mounted controller browser navigates and removes saved links as one reversible edit", async () => {
  const channel = state().project.channels[0].id
  await dispatch({ type: "setControllerLinks", links: [{ source: { type: "midi", control: { type: "cc", number: 1 } }, target: { type: "param", target: { type: "channelPan", channel } }, map: { min: 0, max: 1, smoothingMs: 0, pickup: "inherit", mode: "absolute" } }] })
  const before = state().history.cursor
  render(<ProjectTab />)
  const group = screen.getByRole("region", { name: "Controller links" })
  fireEvent.click(within(group).getByRole("button", { name: /pan/i }))
  expect(useUiStore.getState().selectedChannel).toBe(channel)
  fireEvent.click(screen.getByRole("button", { name: "Remove controller link 1" }))
  await waitFor(() => expect(state().project.settings.controllerLinks ?? []).toHaveLength(0))
  expect(state().history.cursor).toBe(before + 1)
  await act(async () => { await undo() })
  expect(state().project.settings.controllerLinks).toHaveLength(1)
  await app.backend.projectSave("/links.windfall")
  await app.backend.projectOpen("/links.windfall")
  expect(state().project.settings.controllerLinks).toHaveLength(1)
})
it("mounted relationship overview selects the pattern/channel lane and reveals a clip", async () => {
  render(<ProjectOverviewView />)
  const pattern = state().project.patterns[0]
  const channel = state().project.channels[0]
  fireEvent.click(screen.getByRole("button", { name: new RegExp(`Open ${pattern.name} / ${channel.name}`) }))
  await settle()
  expect(useUiStore.getState().centerTab).toBe("pianoRoll")
  expect(useUiStore.getState().selectedChannel).toBe(channel.id)
  expect(useTransportStore.getState().pattern).toBe(pattern.id)
  const clip = state().project.playlist.clips[0]
  if (clip?.content.type === "pattern") {
    fireEvent.click(screen.getAllByRole("button", { name: `Pattern: ${state().project.patterns.find((p) => p.id === (clip.content.type === "pattern" ? clip.content.pattern : -1))?.name}` })[0])
    expect(usePlaylistStore.getState().selection.has(clip.id)).toBe(true)
    expect(usePlaylistStore.getState().reveal).toBe(clip.id)
  }
})
it("mounted catalog inserts a real built-in and undo/save/reopen preserve its state", async () => {
  const before = state().history.cursor
  const count = state().project.channels.length
  render(<PluginCatalog tiles />)
  const tile = await screen.findByRole("region", { name: "Subtractive synth" })
  fireEvent.doubleClick(within(tile).getByRole("img"))
  await waitFor(() => expect(state().project.channels).toHaveLength(count + 1))
  expect(state().history.cursor).toBe(before + 1)
  expect(state().project.channels.at(-1)?.source.type).toBe("instrument")
  await act(async () => { await undo() })
  expect(state().project.channels).toHaveLength(count)
  await act(async () => { await redo() })
  await app.backend.projectSave("/plugin.windfall")
  await app.backend.projectOpen("/plugin.windfall")
  expect(state().project.channels.at(-1)?.source.type).toBe("instrument")
})
it("mounted catalog favorites and categories come from the shared real database", async () => {
  render(<PluginCatalog />)
  fireEvent.click(await screen.findByRole("button", { name: "Favorite Subtractive synth" }))
  await screen.findByRole("button", { name: "Unfavorite Subtractive synth" })
  const category = screen.getByLabelText("Category for Subtractive synth")
  fireEvent.change(category, { target: { value: "My leads" } }); fireEvent.blur(category)
  await waitFor(async () => expect((await app.backend.pluginDatabase()).records.find((r) => r.id === "subtractiveSynth")).toMatchObject({ favorite: true, category: "My leads" }))
})
it("mounted score controls capture/read/apply the real WASM lane in one history step", async () => {
  const channel = useUiStore.getState().selectedChannel!
  const pattern = state().project.patterns[0].id
  const original = state().project.patterns[0].lanes.find((l) => l.channel === channel)!.notes
  render(<ScoreFileControls />)
  fireEvent.click(screen.getByRole("button", { name: "Save lane or selection as score" }))
  await waitFor(() => expect(screen.getByRole("button", { name: "Save lane or selection as score" })).toBeEnabled())
  await act(async () => { await dispatch({ type: "clearLane", pattern, channel }) })
  const before = state().history.cursor
  fireEvent.click(screen.getByRole("button", { name: "Replace lane from score" }))
  await waitFor(() => expect(state().project.patterns[0].lanes.find((l) => l.channel === channel)?.notes.length).toBe(original.length))
  expect(state().history.cursor).toBe(before + 1)
  expect(state().project.patterns[0].lanes.find((l) => l.channel === channel)?.notes.map(({ id: _id, ...note }) => note)).toEqual(original.map(({ id: _id, ...note }) => note))
  await act(async () => { await undo() })
  expect(state().project.patterns[0].lanes.find((l) => l.channel === channel)?.notes ?? []).toHaveLength(0)
  await app.backend.projectSave("/score-history.windfall")
  await app.backend.projectOpen("/score-history.windfall")
  expect(state().project.patterns[0].lanes.find((l) => l.channel === channel)?.notes ?? []).toHaveLength(0)
})
it("mounted score panel refuses a delayed picker after a project replacement", async () => {
  let resolve!: (value: string | null) => void
  vi.spyOn(app.backend, "pickScorePath").mockImplementation(() => new Promise((done) => { resolve = done }))
  const apply = vi.spyOn(app.backend, "scoreApply")
  render(<ScoreFileControls />)
  fireEvent.click(screen.getByRole("button", { name: "Replace lane from score" }))
  await waitFor(() => expect(resolve).toBeTypeOf("function"))
  await act(async () => { await app.backend.projectNew(); resolve("/old.wfscore"); await settle() })
  expect(await screen.findByRole("alert")).toHaveTextContent("project changed")
  expect(apply).not.toHaveBeenCalled()
  expect(state().history.cursor).toBe(0)
})
it("mounted playback settings dispatch the panning law and timebase with history and persistence", async () => {
  const before = state().history.cursor
  render(<ProjectPlaybackSettings />)
  fireEvent.click(screen.getByLabelText("Panning law"))
  fireEvent.click(await screen.findByRole("option", { name: "Linear (+6 dB)" }))
  await waitFor(() => expect(state().project.settings.panLaw).toBe("linear"))
  fireEvent.click(screen.getByLabelText("Project timebase"))
  fireEvent.click(await screen.findByRole("option", { name: "96 PPQ" }))
  await waitFor(() => expect(state().project.settings.timebasePpq).toBe(96))
  expect(state().history.cursor).toBe(before + 2)
  await app.backend.projectSave("/settings.windfall"); await app.backend.projectOpen("/settings.windfall")
  expect(state().project.settings).toMatchObject({ panLaw: "linear", timebasePpq: 96 })
})

function dragData() {
  const data = new Map<string,string>()
  return { types: ["application/x-windfall-plugin"], effectAllowed: "copy", dropEffect: "copy", setData: (type: string,value: string) => data.set(type,value), getData: (type: string) => data.get(type) ?? "" }
}
it("mounted full-screen picker drops a real instrument onto a channel as one reversible saved edit", async () => {
  usePluginPicker.setState({ open: true })
  render(<PluginPicker />)
  const channel = state().project.channels[0]
  const tile = await screen.findByRole("region", { name: "Subtractive synth" })
  const before = state().history.cursor
  const transfer = dragData()
  fireEvent.dragStart(tile, { dataTransfer: transfer })
  fireEvent.drop(screen.getByRole("button", { name: `Drop instrument on ${channel.name}` }), { dataTransfer: transfer })
  await waitFor(() => expect(state().project.channels.find((c) => c.id === channel.id)?.source.type).toBe("instrument"))
  expect(state().history.cursor).toBe(before + 1)
  expect(state().project.channels.find((c) => c.id === channel.id)).toMatchObject({ name: channel.name, mixerTrack: channel.mixerTrack })
  await act(async () => { await undo() })
  expect(state().project.channels.find((c) => c.id === channel.id)?.source).toEqual(channel.source)
  await act(async () => { await redo() })
  await app.backend.projectSave("/picker.windfall"); await app.backend.projectOpen("/picker.windfall")
  expect(state().project.channels.find((c) => c.id === channel.id)?.source.type).toBe("instrument")
})
it("mounted picker searches favorites and persists a supplied image over generated artwork", async () => {
  render(<PluginCatalog tiles />)
  const tile = await screen.findByRole("region", { name: "Subtractive synth" })
  expect(within(tile).getByRole("img")).toHaveAccessibleName(/generated artwork/)
  fireEvent.change(within(tile).getByLabelText("Tile image for Subtractive synth"), { target: { files: [new File(["png fixture"],"lead.png",{ type: "image/png" })] } })
  await waitFor(() => expect(within(tile).getByRole("img")).toHaveAccessibleName("Subtractive synth artwork"))
  expect(localStorage.getItem("windfall.plugin-tile-images.v1")).toContain("data:image/png;base64,")
  fireEvent.click(within(tile).getByRole("button",{ name:"Favorite Subtractive synth" }))
  await within(tile).findByRole("button",{ name:"Unfavorite Subtractive synth" })
  fireEvent.click(screen.getByRole("checkbox", { name: "Favorites only" }))
  fireEvent.change(screen.getByLabelText("Search plugins"),{ target:{ value:"Subtractive synth" } })
  expect(screen.getAllByRole("region")).toHaveLength(1)
  expect(state().history.cursor).toBe(0)
})

it("mounted file browser applies a real Rust channel preset from the memory file library with history and reopen", async () => {
  const added = await dispatch({ type:"addChannel", instrument:"subtractiveSynth" })
  const channel = added.created[0]
  useUiStore.getState().selectChannel(channel)
  const ticket = await app.backend.timelineState()
  const capture = await app.backend.presetCaptureChannel(channel,ticket.generation,ticket.revision)
  const entry = await app.backend.presetLibrarySave("Clean lead",capture)
  await dispatch({ type:"updateChannel",id:channel,patch:{ pan:0.7,volume:0.3 } })
  const before=state().history.cursor
  render(<ProjectFilesPanel />)
  fireEvent.click(await screen.findByRole("button",{name:"Clean lead · channel"}))
  await waitFor(() => expect(state().project.channels.find((c)=>c.id===channel)?.pan).toBe(capture.preset.channel?.pan))
  expect(state().history.cursor).toBe(before+1)
  await act(async()=>{await undo()})
  expect(state().project.channels.find((c)=>c.id===channel)?.pan).toBe(0.7)
  await act(async()=>{await redo()})
  await app.backend.projectSave("/preset.windfall"); await app.backend.projectOpen("/preset.windfall")
  expect(state().project.channels.find((c)=>c.id===channel)?.pan).toBe(capture.preset.channel?.pan)
  expect((await app.backend.presetRead(entry.path)).preset).toEqual({ ...capture.preset, name: "Clean lead" })
})
it("mounted preset file picker refuses a stale project without invoking apply",async()=>{
  const added=await dispatch({type:"addChannel",instrument:"subtractiveSynth"})
  useUiStore.getState().selectChannel(added.created[0])
  let resolve!:(value:string|null)=>void
  vi.spyOn(app.backend,"pickPresetPath").mockImplementation(()=>new Promise((done)=>{resolve=done}))
  const apply=vi.spyOn(app.backend,"presetApplyChannel")
  render(<ProjectFilesPanel />)
  fireEvent.click(screen.getByRole("button",{name:"Browse preset files on disk…"}))
  await waitFor(()=>expect(resolve).toBeTypeOf("function"))
  await act(async()=>{await app.backend.projectNew();resolve("/stale.wfpreset");await settle()})
  await waitFor(()=>expect(screen.getAllByRole("alert").some((alert)=>alert.textContent?.includes("project changed"))).toBe(true))
  expect(apply).not.toHaveBeenCalled();expect(state().history.cursor).toBe(0)
})

it("mounted picker drops an effect onto a mixer slot with undo and persistence", async () => {
  usePluginPicker.setState({ open: true })
  render(<PluginPicker />)
  const track = state().project.mixer.tracks[0]
  const catalog = await app.backend.pluginDatabase()
  const effect = catalog.records.find((record) => record.format === "builtin" && !record.instrument)!
  const tile = await screen.findByRole("region", { name: effect.name })
  const transfer = dragData()
  fireEvent.dragStart(tile, { dataTransfer: transfer })
  const before = state().history.cursor
  fireEvent.drop(screen.getByRole("button", { name: `Drop effect on ${track.name} new slot` }), { dataTransfer: transfer })
  await waitFor(() => expect(state().project.mixer.tracks.find((t) => t.id === track.id)?.effects).toHaveLength(track.effects.length + 1))
  expect(state().history.cursor).toBe(before + 1)
  await act(async () => { await undo() })
  expect(state().project.mixer.tracks.find((t) => t.id === track.id)?.effects).toHaveLength(track.effects.length)
  await act(async () => { await redo() })
  await app.backend.projectSave("/picker-effect.windfall"); await app.backend.projectOpen("/picker-effect.windfall")
  expect(state().project.mixer.tracks.find((t) => t.id === track.id)?.effects.at(-1)?.params.type).toBe(effect.id)
})
it("mounted disk browser applies a real built-in instrument plugin preset and reopens it", async () => {
  const added = await dispatch({ type: "addChannel", instrument: "subtractiveSynth" })
  const channel = added.created[0]
  useUiStore.getState().selectChannel(channel)
  const ticket = await app.backend.timelineState()
  const capture = await app.backend.presetCapturePlugin({ type: "instrument", channel }, ticket.generation, ticket.revision)
  await app.backend.presetWrite("/instrument.wfpreset", capture)
  const saved = state().project.channels.find((c) => c.id === channel)!.source
  const destination = state().project.channels.find((c) => c.id === channel)!
  await dispatch({ type: "applyChannelPreset", id: channel, expected: destination, nameColor: false, preset: { name: destination.name, color: destination.color, volume: destination.volume, pan: destination.pan, source: { type: "instrument", params: instrumentDescriptor("fourOp").defaults } } })
  // A selected mixer effect must not steal an instrument preset destination.
  const effect = await dispatch({ type: "addEffect", track: state().project.mixer.tracks[0].id, kind: "delay" })
  openEffect(effect.created[0])
  const before = state().history.cursor
  render(<ProjectFilesPanel />)
  fireEvent.click(screen.getByRole("button", { name: "Browse preset files on disk…" }))
  await waitFor(() => expect(state().project.channels.find((c) => c.id === channel)?.source).toEqual(saved))
  expect(state().history.cursor).toBe(before + 1)
  await act(async () => { await undo() })
  expect(state().project.channels.find((c) => c.id === channel)?.source).not.toEqual(saved)
  await act(async () => { await redo() })
  await app.backend.projectSave("/instrument-preset.windfall"); await app.backend.projectOpen("/instrument-preset.windfall")
  expect(state().project.channels.find((c) => c.id === channel)?.source).toEqual(saved)
})

it.each([...PROJECT_TIMEBASES, 1, 123, 32767])("mounted MIDI export emits the requested %i PPQ in actual SMF bytes", async (ppq) => {
  const original = state().project
  const history = state().history.cursor
  useUiStore.getState().openDialog("midiExport")
  render(<MidiDialogs />)
  if (PROJECT_TIMEBASES.includes(ppq)) {
    fireEvent.click(screen.getByRole("combobox", { name: "Resolution" }))
    fireEvent.click(await screen.findByRole("option", { name: `${ppq} PPQ` }))
  } else {
    fireEvent.change(screen.getByRole("spinbutton", { name: "Custom MIDI resolution (1–32767)" }), { target: { value: String(ppq) } })
  }
  fireEvent.click(screen.getByRole("button", { name: "Choose file and export…" }))
  await waitFor(() => expect(midiFiles).toHaveLength(1))
  const bytes = midiFiles[0]
  expect([...bytes.slice(0, 4)]).toEqual([77, 84, 104, 100])
  expect(new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint16(12)).toBe(ppq)
  expect(state().project).toEqual(original)
  expect(state().history.cursor).toBe(history)
})

it("mounted picker refuses a drag after an intervening document edit", async () => {
  usePluginPicker.setState({ open: true })
  render(<PluginPicker />)
  const channel = state().project.channels[0]
  const tile = await screen.findByRole("region", { name: "Subtractive synth" })
  const transfer = dragData()
  fireEvent.dragStart(tile, { dataTransfer: transfer })
  await act(async () => { await dispatch({ type: "updateSettings", patch: { name: "Changed during drag" } }) })
  const before = state().history.cursor
  const error = vi.spyOn(toast, "error")
  fireEvent.drop(screen.getByRole("button", { name: `Drop instrument on ${channel.name}` }), { dataTransfer: transfer })
  await waitFor(() => expect(error).toHaveBeenCalledWith("Could not add plugin", { description: "The project changed during the plugin drag" }))
  expect(state().history.cursor).toBe(before)
  expect(state().project.channels.find((c) => c.id === channel.id)?.source).toEqual(channel.source)
})
