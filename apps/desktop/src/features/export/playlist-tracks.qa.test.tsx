import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import type { Project } from "@/bindings"
import { usePlaylistStore } from "@/features/playlist/store"
import { demoProject } from "@/lib/ipc/sim/project"
import { playlistStemTracks } from "@/lib/ipc/sim/playlist"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { settle, startTestApp } from "@/test/harness"

import { ExportDialog } from "./export-dialog"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

/*
 * Export of the playlist tracks from the mounted export dialog: what the
 * form sends, how it keeps the two kinds of stem apart, and how a refusal
 * of the backend is shown. The files themselves are written and read back
 * natively, in session/tests/playlist_export.rs.
 */

let app: Awaited<ReturnType<typeof startTestApp>>
let source: Project

beforeEach(async () => {
  source = demoProject()
  source.nextId = 1000
  source.playlist.tracks = [
    { id: 900, name: "Drums", muted: false, solo: false },
    { id: 901, name: "Bass", muted: false, solo: false },
    { id: 902, name: "Spare", muted: false, solo: false },
  ]
  const pattern = { type: "pattern" as const, pattern: source.patterns[0].id }
  source.playlist.clips = [
    { id: 910, track: 900, start: 0, length: 3840, offset: 0, muted: false, content: pattern },
    { id: 911, track: 901, start: 3840, length: 3840, offset: 0, muted: false, content: pattern },
  ]
  app = await startTestApp({ project: source })
  usePlaylistStore.setState(usePlaylistStore.getInitialState(), true)
  useUiStore.getState().openDialog("export")
  render(<ExportDialog />)
})

afterEach(() => {
  app.stop()
  vi.restoreAllMocks()
})

const lanes = () =>
  screen.getByRole("checkbox", {
    name: "Export each playlist track to its own file",
  })
const mixerStems = () =>
  screen.getByRole("checkbox", { name: "Export mixer tracks as stems" })

async function submit(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "Choose…" }))
  await user.click(screen.getByRole("button", { name: "Export" }))
  await settle()
}

it("sends every playlist track of the song, apart from the mixer stems", async () => {
  const user = userEvent.setup()
  const plain = vi.spyOn(app.backend, "exportAudio").mockResolvedValue(undefined)
  const apart = vi
    .spyOn(app.backend, "exportPlaylistTracks")
    .mockResolvedValue(undefined)
  // The two kinds of stem take each other's place, whichever is turned on.
  await user.click(lanes())
  expect(lanes()).toBeChecked()
  await user.click(mixerStems())
  expect(mixerStems()).toBeChecked()
  expect(lanes()).not.toBeChecked()
  await user.click(lanes())
  expect(lanes()).toBeChecked()
  expect(mixerStems()).not.toBeChecked()
  expect(screen.getByRole("combobox", { name: "Render" })).toHaveTextContent(
    "The whole song"
  )

  await submit(user)

  await waitFor(() => expect(apart).toHaveBeenCalledOnce())
  const [options, stems] = apart.mock.calls[0]
  expect(options).toMatchObject({ mode: "song", format: "wav" })
  expect(options.stems).toBeUndefined()
  expect(options.path).not.toBe("")
  expect(stems).toEqual({ includeMix: true, numbered: true, folder: true })
  expect(plain).not.toHaveBeenCalled()
})

it("sends the chosen tracks and the file options", async () => {
  const user = userEvent.setup()
  const apart = vi
    .spyOn(app.backend, "exportPlaylistTracks")
    .mockResolvedValue(undefined)
  await user.click(lanes())
  // The lanes of the clips that are selected on the playlist.
  act(() => usePlaylistStore.getState().select([911]))
  await user.click(
    screen.getByRole("button", { name: "Use tracks of selected clips (1)" })
  )
  expect(screen.getByRole("checkbox", { name: "Bass" })).toBeChecked()
  expect(screen.getByRole("checkbox", { name: "Drums" })).not.toBeChecked()
  await user.click(screen.getByRole("checkbox", { name: "Drums" }))
  await user.click(
    screen.getByRole("checkbox", { name: "Number files in playlist order" })
  )
  await user.click(
    screen.getByRole("checkbox", { name: "Put the files in their own folder" })
  )
  await submit(user)
  await waitFor(() => expect(apart).toHaveBeenCalledOnce())
  expect(apart.mock.calls[0][1]).toEqual({
    tracks: [901, 900],
    includeMix: true,
    numbered: false,
    folder: false,
  })
})

it("asks for a track or the mix, and for the song", async () => {
  const user = userEvent.setup()
  const apart = vi
    .spyOn(app.backend, "exportPlaylistTracks")
    .mockResolvedValue(undefined)
  await user.click(lanes())
  await user.click(screen.getByRole("combobox", { name: "Tracks to export" }))
  await user.click(await screen.findByRole("option", { name: "Choose tracks" }))
  for (const name of ["Drums", "Bass", "Spare"]) {
    await user.click(screen.getByRole("checkbox", { name }))
  }
  // The second "Include the full mix" belongs to the playlist tracks.
  await user.click(screen.getByLabelText("Include the full mix"))
  await submit(user)
  expect(
    screen.getByText("Choose at least one playlist track or include the full mix.")
  ).toBeInTheDocument()
  expect(apart).not.toHaveBeenCalled()

  await user.click(screen.getByRole("checkbox", { name: "Bass" }))
  await user.click(screen.getByRole("combobox", { name: "Render" }))
  await user.click(
    await screen.findByRole("option", { name: "The selected pattern" })
  )
  await user.click(screen.getByRole("button", { name: "Export" }))
  expect(
    screen.getByText(
      "Playlist tracks belong to the song. Render the whole song to export them."
    )
  ).toBeInTheDocument()
  expect(apart).not.toHaveBeenCalled()
})

it("shows why the backend refused, and exports when the lanes can give something", async () => {
  const user = userEvent.setup()
  const plain = vi.spyOn(app.backend, "exportAudio")
  await act(async () => {
    await dispatch({
      type: "updatePlaylistTrack",
      id: 901,
      patch: { muted: true },
    })
  })
  // What the simulated backend would export follows mute and empty lanes.
  const project = useProjectStore.getState().project
  expect(
    playlistStemTracks(project, { includeMix: true, numbered: true, folder: true })
  ).toEqual([900])
  await user.click(lanes())
  act(() => usePlaylistStore.getState().select([911]))
  await user.click(
    screen.getByRole("button", { name: "Use tracks of selected clips (1)" })
  )
  await submit(user)
  expect(
    await screen.findByText(/Playlist track 901 plays nothing/)
  ).toBeInTheDocument()
  expect(plain).not.toHaveBeenCalled()

  // Every lane with sound: the muted one is left out, and the export runs.
  await user.click(screen.getByRole("combobox", { name: "Tracks to export" }))
  await user.click(
    await screen.findByRole("option", { name: "All tracks that have sound" })
  )
  await user.click(screen.getByRole("button", { name: "Export" }))
  await waitFor(() => expect(plain).toHaveBeenCalledOnce())
  expect(plain.mock.calls[0][0].stems).toBeUndefined()
})
