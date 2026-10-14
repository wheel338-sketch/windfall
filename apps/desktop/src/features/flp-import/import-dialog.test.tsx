import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { PromptHost } from "@/features/layout/prompt-host"
import type { FlpImportPreview } from "@/bindings"
import type { MockBackend } from "@/lib/ipc/mock"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { startTestApp } from "@/test/harness"
import { FlpImportDialog } from "./import-dialog"
import { ImportedSoundsButton, RetainedSoundsDialog } from "./retained-dialog"
import { flpFixture, flpOmittedFixture } from "./fixture"
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }),
}))
let backend: MockBackend
let stop: () => void
beforeEach(async () => {
  ;({ backend, stop } = await startTestApp({
    flpFiles: {
      "/projects/source.flp": flpFixture(),
      "/projects/omitted.flp": flpOmittedFixture(),
      "/projects/bad.flp": new Uint8Array([1, 2]),
    },
  }))
  vi.spyOn(backend, "pickFlpFile").mockResolvedValue("/projects/source.flp")
  useUiStore.getState().openDialog("flpImport")
  render(
    <>
      <FlpImportDialog />
      <PromptHost />
      <ImportedSoundsButton />
      <RetainedSoundsDialog />
    </>
  )
})
afterEach(() => {
  stop()
  vi.restoreAllMocks()
})
async function choose() {
  await userEvent.click(
    screen.getByRole("button", { name: /Choose FL project/ })
  )
  await screen.findByRole("region", { name: "Import report" })
}
it("reviews original bytes without replacement, then opens an unsaved checked project and saves retained data", async () => {
  const before = await backend.documentSnapshot()
  await choose()
  expect(await backend.documentSnapshot()).toEqual(before)
  expect(screen.getByText("1 missing samples")).toBeVisible()
  expect(screen.getByText("1 retained plugin states")).toBeVisible()
  await userEvent.click(
    screen.getByRole("button", { name: /Diagnostics and missing samples/ })
  )
  expect(screen.getByText(/Missing sample:/)).toBeVisible()
  await userEvent.click(
    screen.getByRole("button", { name: "Open converted project" })
  )
  await waitFor(() => expect(useUiStore.getState().dialog).toBeNull())
  const converted = await backend.documentSnapshot()
  expect(converted.dirty).toBe(true)
  expect(converted.path).toBeNull()
  expect(converted.project.patterns[0]?.lanes[0]?.notes).toHaveLength(1)
  expect(converted.project.retainedPlugins?.[0]?.state).toEqual([
    0, 255, 17, 90,
  ])
  const saved = await backend.projectSave("/projects/converted.windfall")
  await backend.projectNew()
  const reloaded = await backend.projectOpen(saved)
  expect(reloaded.project.retainedPlugins).toEqual(
    converted.project.retainedPlugins
  )
  await userEvent.click(
    screen.getByRole("button", { name: "Imported sounds (1)" })
  )
  await userEvent.click(screen.getByRole("button", { name: "Unknown synth" }))
  expect(screen.getByText("4 original state bytes retained.")).toBeVisible()
})
it("names the omitted controls of each device in the review, and says nothing when none are", async () => {
  // The fixture project uses no device with an omitted control.
  await choose()
  expect(screen.queryByRole("region", { name: "Omitted controls" })).toBeNull()
  // The same review with what the importer reports for two limiters and a
  // pitch corrector, and one device beyond what a report lists.
  const preview = backend.flpPreview.bind(backend)
  const reported = vi.spyOn(backend, "flpPreview").mockImplementation(async (path, options) => {
    const result = await preview(path, options)
    return {
      ...result,
      report: {
        ...result.report,
        omittedControls: [
          { device: "Fruity Limiter", instances: 2, fields: ["ceiling", "knee"] },
          { device: "Pitcher", instances: 1, fields: ["speed"] },
        ],
        moreOmittedDevices: 1,
      },
    }
  })
  await userEvent.click(screen.getByRole("button", { name: "Review again" }))
  const omitted = await screen.findByRole("region", { name: "Omitted controls" })
  const items = within(omitted).getAllByRole("listitem")
  expect(items.map((item) => item.textContent)).toEqual([
    "Fruity Limiter × 2: ceiling, knee",
    "Pitcher: speed",
    "1 more device has omitted controls.",
  ])
  expect(omitted).toHaveTextContent("original state is kept with the project")
  reported.mockRestore()
})

it("reviews a project the importer really converted: its counts and its omitted controls", async () => {
  // A SimSynth channel and a soft clipper on the master, run through the
  // importer itself (the simulator's build of it). Nothing of this report
  // is made up here: the review shows what the converter reported.
  vi.mocked(backend.pickFlpFile).mockResolvedValue("/projects/omitted.flp")
  const previews = vi.spyOn(backend, "flpPreview")
  await choose()
  expect(previews).toHaveBeenCalledTimes(1)
  const preview = (await previews.mock.results[0]?.value) as FlpImportPreview
  const { report } = preview
  const shown = report.categories.filter(
    (c) => c.exact + c.approximated + c.placeholders + c.dropped > 0
  )
  expect(shown.length).toBeGreaterThan(0)
  const escaped = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
  for (const category of shown) {
    const trigger = screen.getByRole("button", {
      name: new RegExp(`^${escaped(category.title)}`),
    })
    expect(trigger).toHaveTextContent(
      `${category.exact} exact · ${category.approximated} approximate · ${category.placeholders} placeholders · ${category.dropped} omitted`
    )
  }
  // Each omitted field is a dropped line of its category, not part of an
  // approximated one: the clipper's post gain among the effects, the
  // synth's five switches among the channels.
  const omittedLines = (section: string) =>
    report.categories
      .find((c) => c.section === section)
      ?.lines.filter(
        (line) => line.outcome === "dropped" && line.text.includes("omitted:")
      ).length ?? 0
  expect(omittedLines("effects")).toBe(1)
  expect(omittedLines("channels")).toBe(5)
  const omitted = screen.getByRole("region", { name: "Omitted controls" })
  const items = within(omitted)
    .getAllByRole("listitem")
    .map((item) => item.textContent)
    .sort()
  expect(items).toEqual([
    "Fruity Soft Clipper: postgain",
    "SimSynth: osc1_warm, osc2_warm, osc3_warm, lfo_trackamp, chorus_on",
  ])
  expect(report.omittedControls.map((device) => device.device).sort()).toEqual([
    "Fruity Soft Clipper",
    "SimSynth",
  ])
  expect(report.moreOmittedDevices).toBe(0)
  previews.mockRestore()
})

it("cancels a review and an FL picker without changing the active document", async () => {
  const before = await backend.documentSnapshot()
  vi.mocked(backend.pickFlpFile).mockResolvedValueOnce(null)
  await userEvent.click(
    screen.getByRole("button", { name: /Choose FL project/ })
  )
  expect(screen.queryByRole("region", { name: "Import report" })).toBeNull()
  await choose()
  await userEvent.click(screen.getByRole("button", { name: "Cancel" }))
  await waitFor(() => expect(useUiStore.getState().dialog).toBeNull())
  expect(await backend.documentSnapshot()).toEqual(before)
})
it("reports malformed files and preserves the active project", async () => {
  const before = await backend.documentSnapshot()
  vi.mocked(backend.pickFlpFile).mockResolvedValueOnce("/projects/bad.flp")
  await userEvent.click(
    screen.getByRole("button", { name: /Choose FL project/ })
  )
  expect(await screen.findByRole("alert")).toHaveTextContent(/FL/)
  expect(await backend.documentSnapshot()).toEqual(before)
  expect(
    screen.getByRole("button", { name: "Open converted project" })
  ).toBeDisabled()
})
it("uses the existing unsaved prompt and leaves edits intact when replacement is declined", async () => {
  await act(async () => {
    await dispatch({ type: "updateSettings", patch: { name: "Keep me" } })
  })
  await choose()
  await userEvent.click(
    screen.getByRole("button", { name: "Open converted project" })
  )
  await screen.findByRole("alertdialog")
  await userEvent.click(screen.getByRole("button", { name: "Cancel" }))
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull())
  expect(useProjectStore.getState().project.settings.name).toBe("Keep me")
  expect(
    screen.getByRole("button", { name: "Open converted project" })
  ).toBeEnabled()
})
it("rejects a stale preview after intervening edits", async () => {
  await choose()
  const preview = await backend.flpPreview("/projects/source.flp", {
    factoryDataDir: null,
    userDataDir: null,
    sampleSearchFolders: [],
  })
  await backend.dispatch({
    type: "updateSettings",
    patch: { name: "New edit" },
  })
  const before = await backend.documentSnapshot()
  await expect(backend.flpOpen(preview.token)).rejects.toThrow(/changed/)
  expect(await backend.documentSnapshot()).toEqual(before)
})

it("adds and removes search folders while keeping the active document intact", async () => {
  const before = await backend.documentSnapshot()
  await choose()
  const previews = vi.spyOn(backend, "flpPreview")
  vi.spyOn(backend, "pickFolder").mockResolvedValue("/samples/Test")
  await userEvent.click(
    screen.getByRole("button", { name: "Sample folders (optional)" })
  )
  await userEvent.click(
    screen.getByRole("button", { name: /Add sample search folder/ })
  )
  await waitFor(() =>
    expect(previews).toHaveBeenLastCalledWith(
      "/projects/source.flp",
      expect.objectContaining({ sampleSearchFolders: ["/samples/Test"] })
    )
  )
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Remove Test" })).toBeEnabled()
  )
  await userEvent.click(screen.getByRole("button", { name: "Remove Test" }))
  await waitFor(() =>
    expect(previews).toHaveBeenLastCalledWith(
      "/projects/source.flp",
      expect.objectContaining({ sampleSearchFolders: [] })
    )
  )
  expect(await backend.documentSnapshot()).toEqual(before)
})
