import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { MixerToolbar } from "@/features/mixer/toolbar"
import { useMixerUi } from "@/features/mixer/mixer-ui"
import { trackNamed } from "@/features/mixer/test-utils"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { announceProjectReplaced } from "@/lib/store/replaced"
import { startTestApp } from "@/test/harness"
import { ExportDialog } from "./export-dialog"
import { useMixerRender } from "./mixer-render"

let app: Awaited<ReturnType<typeof startTestApp>>
beforeEach(async () => {
  app = await startTestApp()
  useMixerUi.setState({ selected: [], anchor: null })
  useMixerRender.setState({ request: null })
})
afterEach(() => {
  app.stop()
  vi.restoreAllMocks()
})

function mount() {
  return render(
    <>
      <MixerToolbar filter="" onFilterChange={() => {}} />
      <ExportDialog />
    </>
  )
}
async function submit(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "Choose…" }))
  await user.click(screen.getByRole("button", { name: "Export" }))
}

it("selected inserts plus Master submit captured output stems while Current is excluded", async () => {
  const user = userEvent.setup()
  const kick = trackNamed("Kick")
  await act(async () => {
    await dispatch({ type: "ensureCurrentMixerTrack" })
  })
  const current = useProjectStore
    .getState()
    .project.mixer.tracks.find((track) => track.current)!
  useMixerUi.setState({ selected: [0, kick.id, current.id] })
  const exported = vi
    .spyOn(app.backend, "exportAudio")
    .mockResolvedValue(undefined)
  mount()
  await user.click(screen.getByRole("button", { name: "Render selected…" }))
  expect(
    screen.getByRole("checkbox", { name: "Export mixer tracks as stems" })
  ).toBeChecked()
  expect(
    screen.queryByRole("checkbox", { name: current.name })
  ).not.toBeInTheDocument()
  await submit(user)
  await waitFor(() => expect(exported).toHaveBeenCalledOnce())
  expect(exported).toHaveBeenCalledWith(
    expect.objectContaining({
      stems: {
        mode: "trackOutputs",
        tracks: [kick.id],
        includeMix: true,
        numbered: true,
        folder: true,
      },
    }),
    expect.stringMatching(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
  )
  expect(useMixerRender.getState().request).toBeNull()
})

it("saved armed inserts and Master open the matching offline export request without mutating arms", async () => {
  const user = userEvent.setup()
  const hat = trackNamed("Hat")
  const recording = {
    input: null,
    armed: true,
    monitor: false,
    monitorGain: 1,
    monitorBufferMs: 20,
    offsetMs: 0,
    mode: "postFader" as const,
  }
  await act(async () => {
    await dispatch({
      type: "batch",
      commands: [0, hat.id].map((id) => ({
        type: "updateMixerTrack" as const,
        id,
        patch: { recording },
      })),
    })
  })
  const before = useProjectStore
    .getState()
    .project.mixer.tracks.map((track) => track.recording)
  const exported = vi
    .spyOn(app.backend, "exportAudio")
    .mockResolvedValue(undefined)
  mount()
  await user.click(screen.getByRole("button", { name: "Render armed…" }))
  await submit(user)
  await waitFor(() => expect(exported).toHaveBeenCalledOnce())
  expect(exported).toHaveBeenCalledWith(
    expect.objectContaining({
      stems: expect.objectContaining({
        tracks: [hat.id],
        includeMix: true,
        mode: "trackOutputs",
      }),
    }),
    expect.stringMatching(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
  )
  expect(
    useProjectStore
      .getState()
      .project.mixer.tracks.map((track) => track.recording)
  ).toEqual(before)
})

it.each(["removed", "replaced"] as const)(
  "rejects a %s captured source before submitting",
  async (change) => {
    const user = userEvent.setup()
    const kick = trackNamed("Kick")
    useMixerUi.setState({ selected: [kick.id] })
    const exported = vi
      .spyOn(app.backend, "exportAudio")
      .mockResolvedValue(undefined)
    mount()
    await user.click(screen.getByRole("button", { name: "Render selected…" }))
    await user.click(screen.getByRole("button", { name: "Choose…" }))
    await act(async () => {
      if (change === "removed")
        await dispatch({ type: "removeMixerTrack", id: kick.id })
      else {
        // Same-numbered ids still exist: generation identity must reject them.
        announceProjectReplaced()
        useProjectStore.setState((state) => ({
          project: {
            ...state.project,
            mixer: {
              ...state.project.mixer,
              tracks: [...state.project.mixer.tracks],
            },
          },
        }))
      }
    })
    expect(
      screen.getByText(
        change === "removed"
          ? /An export track was removed/
          : /The project was replaced/
      )
    ).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Export" }))
    expect(exported).not.toHaveBeenCalled()
  }
)

it("surfaces native export admission errors for the selected stem request", async () => {
  const user = userEvent.setup()
  const kick = trackNamed("Kick")
  useMixerUi.setState({ selected: [kick.id] })
  const exported = vi
    .spyOn(app.backend, "exportAudio")
    .mockRejectedValue(new Error("Selected stem source is unavailable"))
  mount()
  await user.click(screen.getByRole("button", { name: "Render selected…" }))
  await submit(user)
  expect(
    await screen.findByText(/Selected stem source is unavailable/)
  ).toBeInTheDocument()
  expect(exported).toHaveBeenCalledWith(
    expect.objectContaining({
      stems: expect.objectContaining({ tracks: [kick.id], includeMix: false }),
    }),
    expect.stringMatching(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
  )
})
