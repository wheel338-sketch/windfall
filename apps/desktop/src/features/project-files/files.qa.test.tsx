import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { startTestApp, settle } from "@/test/harness"
import { dispatch, undo, useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { ProjectTab } from "@/features/browser/project-tab"
import { ProjectOverviewView } from "@/features/project-overview/view"
import { PluginCatalog } from "@/features/plugins/catalog"
import { ProjectPlaybackSettings } from "@/features/project-info/settings"
import { ScoreFileControls } from "./score-controls"
import { usePlaylistStore } from "@/features/playlist/store"
import { useTransportStore } from "@/lib/store/transport"

let app: Awaited<ReturnType<typeof startTestApp>>
beforeEach(async () => { app = await startTestApp(); useUiStore.getState().selectChannel(useProjectStore.getState().project.channels[0].id) })
afterEach(() => { app.stop(); vi.restoreAllMocks() })
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
