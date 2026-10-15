import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"

import type { Notebook } from "@/bindings"
import { applyPatch } from "@/lib/store/patch"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { announceProjectReplaced } from "@/lib/store/replaced"

import { NotebookControls } from "./index"
import { NOTEBOOK_COMMIT_DELAY_MS, NotebookPanel } from "./panel"

vi.mock("@/lib/store/project", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/store/project")>()),
  dispatch: vi.fn(),
}))

const original = useProjectStore.getState()
const blank = () => ({ title: "", body: "" })
const saveButton = () => screen.getByRole("button", { name: "Save notebook" })
const form = () => screen.getByRole("form", { name: "Song notebook" })

afterEach(() => {
  useProjectStore.setState(original)
  vi.resetAllMocks()
})

async function choosePage(name: string) {
  const user = userEvent.setup()
  await user.click(screen.getByRole("combobox", { name: "Page" }))
  await user.click(await screen.findByRole("option", { name }))
}

describe("song notebook", () => {
  it("shows an omitted book as a temporary blank without dispatching", async () => {
    useProjectStore.setState({
      project: { ...original.project, notebook: undefined },
    })
    render(<NotebookControls />)
    fireEvent.click(screen.getByRole("button", { name: "Notebook" }))
    expect(await screen.findByLabelText("Page title")).toHaveValue("")
    expect(screen.getByLabelText("Page body")).toHaveValue("")
    expect(saveButton()).toBeDisabled()
    expect(screen.getByRole("button", { name: "Remove page" })).toBeDisabled()
    expect(
      screen.getByRole("button", { name: "Duplicate page" })
    ).toBeDisabled()
    fireEvent.submit(form())
    expect(dispatch).not.toHaveBeenCalled()
  })

  it("dispatches one whole-book command for edits across pages", async () => {
    useProjectStore.setState({
      project: { ...original.project, notebook: undefined },
    })
    vi.mocked(dispatch).mockResolvedValue(null)
    render(<NotebookControls />)
    fireEvent.click(screen.getByRole("button", { name: "Notebook" }))
    fireEvent.change(await screen.findByLabelText("Page title"), {
      target: { value: "  Verse  " },
    })
    fireEvent.change(screen.getByLabelText("Page body"), {
      target: { value: "  First line\nSecond line  " },
    })
    fireEvent.click(screen.getByRole("button", { name: "Add page" }))
    fireEvent.change(screen.getByLabelText("Page title"), {
      target: { value: " Mix " },
    })
    fireEvent.change(screen.getByLabelText("Page body"), {
      target: { value: "<script>plain text</script>" },
    })
    await choosePage("1. Verse")
    expect(screen.getByLabelText("Page body")).toHaveValue(
      "  First line\nSecond line  "
    )
    expect(dispatch).not.toHaveBeenCalled()
    fireEvent.click(saveButton())
    await waitFor(() => expect(dispatch).toHaveBeenCalledTimes(1))
    expect(dispatch).toHaveBeenCalledWith(
      {
        type: "replaceNotebook",
        notebook: {
          pages: [
            { title: "Verse", body: "  First line\nSecond line  " },
            { title: "Mix", body: "<script>plain text</script>" },
          ],
        },
      },
      expect.any(Number)
    )
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not save notebook."
    )
    expect(screen.getByLabelText("Page title")).toHaveValue("  Verse  ")
  })

  it("adds up to eight pages and removes the current page without losing others", async () => {
    const onSave = vi.fn(async () => true)
    render(
      <NotebookPanel
        notebook={{ pages: [{ title: "First", body: "Keep" }] }}
        onSave={onSave}
      />
    )
    const add = screen.getByRole("button", { name: "Add page" })
    for (let index = 1; index < 8; index++) fireEvent.click(add)
    expect(add).toBeDisabled()
    expect(
      screen.getByRole("button", { name: "Duplicate page" })
    ).toBeDisabled()
    expect(screen.getByText("8 of 8 pages")).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Remove page" }))
    expect(add).toBeEnabled()
    expect(screen.getByRole("button", { name: "Duplicate page" })).toBeEnabled()
    await choosePage("1. First")
    expect(screen.getByLabelText("Page body")).toHaveValue("Keep")
    fireEvent.click(saveButton())
    await waitFor(() =>
      expect(onSave).toHaveBeenCalledWith({
        pages: [
          { title: "First", body: "Keep" },
          ...Array.from({ length: 6 }, blank),
        ],
      })
    )
    expect(saveButton()).toBeDisabled()
  })

  it("duplicates the selected draft page, selects the copy, and waits for Save notebook", async () => {
    const onSave = vi.fn(async () => true)
    render(
      <NotebookPanel
        notebook={{
          pages: [
            { title: "Verse", body: "Original" },
            { title: "Chorus", body: "Refrain" },
            { title: "Bridge", body: "Ending" },
          ],
        }}
        onSave={onSave}
      />
    )
    await choosePage("2. Chorus")
    fireEvent.change(screen.getByLabelText("Page body"), {
      target: { value: "  Draft refrain\nSecond line  " },
    })
    fireEvent.click(screen.getByRole("button", { name: "Duplicate page" }))

    expect(screen.getByRole("combobox", { name: "Page" })).toHaveTextContent(
      "3. Chorus"
    )
    expect(screen.getByLabelText("Page title")).toHaveValue("Chorus")
    expect(screen.getByLabelText("Page body")).toHaveValue(
      "  Draft refrain\nSecond line  "
    )
    expect(screen.getByText("4 of 8 pages")).toBeInTheDocument()
    expect(onSave).not.toHaveBeenCalled()

    fireEvent.change(screen.getByLabelText("Page title"), {
      target: { value: "Chorus copy" },
    })
    fireEvent.click(saveButton())
    await waitFor(() =>
      expect(onSave).toHaveBeenCalledWith({
        pages: [
          { title: "Verse", body: "Original" },
          { title: "Chorus", body: "  Draft refrain\nSecond line  " },
          { title: "Chorus copy", body: "  Draft refrain\nSecond line  " },
          { title: "Bridge", body: "Ending" },
        ],
      })
    )
    expect(onSave).toHaveBeenCalledTimes(1)
  })

  it("removes the last page by saving an empty book", async () => {
    const onSave = vi.fn(async () => true)
    render(
      <NotebookPanel
        notebook={{ pages: [{ title: "Last", body: "Notes" }] }}
        onSave={onSave}
      />
    )
    fireEvent.click(screen.getByRole("button", { name: "Remove page" }))
    expect(screen.getByLabelText("Page title")).toHaveValue("")
    expect(screen.getByLabelText("Page body")).toHaveValue("")
    expect(
      screen.getByRole("button", { name: "Duplicate page" })
    ).toBeDisabled()
    fireEvent.click(saveButton())
    await waitFor(() => expect(onSave).toHaveBeenCalledWith({ pages: [] }))
  })

  it.each([
    ["Page title", 128],
    ["Page body", 16_384],
  ])(
    "validates %s in UTF-8 bytes and blocks an invalid hidden page",
    async (label, cap) => {
      const onSave = vi.fn(async () => true)
      render(<NotebookPanel onSave={onSave} />)
      fireEvent.change(screen.getByLabelText(label), {
        target: { value: "é".repeat(cap / 2) },
      })
      expect(saveButton()).toBeEnabled()
      fireEvent.change(screen.getByLabelText(label), {
        target: { value: "é".repeat(cap / 2) + "x" },
      })
      expect(screen.getByLabelText(label)).toHaveAttribute(
        "aria-invalid",
        "true"
      )
      expect(saveButton()).toBeDisabled()
      fireEvent.click(screen.getByRole("button", { name: "Add page" }))
      expect(screen.getByRole("alert")).toHaveTextContent(
        "Page 1 exceeds a text limit."
      )
      fireEvent.submit(form())
      expect(onSave).not.toHaveBeenCalled()
    }
  )

  it("trims only titles and counts body whitespace against its cap", () => {
    render(<NotebookPanel onSave={vi.fn(async () => true)} />)
    fireEvent.change(screen.getByLabelText("Page title"), {
      target: { value: `  ${"é".repeat(64)}  ` },
    })
    expect(saveButton()).toBeEnabled()
    fireEvent.change(screen.getByLabelText("Page body"), {
      target: { value: "é".repeat(8192) + " " },
    })
    expect(screen.getByLabelText("Page body")).toHaveAttribute(
      "aria-invalid",
      "true"
    )
    expect(saveButton()).toBeDisabled()
  })

  it("disables controls and prevents duplicate commands while saving", async () => {
    let finish: (value: boolean) => void = () => {}
    const onSave = vi.fn(
      () =>
        new Promise<boolean>((resolve) => {
          finish = resolve
        })
    )
    render(<NotebookPanel onSave={onSave} />)
    fireEvent.change(screen.getByLabelText("Page body"), {
      target: { value: "Notes" },
    })
    fireEvent.submit(form())
    expect(screen.getByLabelText("Page title")).toBeDisabled()
    expect(screen.getByLabelText("Page body")).toBeDisabled()
    expect(screen.getByRole("combobox", { name: "Page" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Add page" })).toBeDisabled()
    expect(
      screen.getByRole("button", { name: "Duplicate page" })
    ).toBeDisabled()
    fireEvent.submit(form())
    expect(onSave).toHaveBeenCalledTimes(1)
    await act(async () => finish(true))
    expect(saveButton()).toBeDisabled()
  })

  it("applies saved and undo patches to the mirror, including clearing all pages", async () => {
    useProjectStore.setState({
      project: { ...original.project, notebook: undefined },
    })
    render(<NotebookControls />)
    fireEvent.click(screen.getByRole("button", { name: "Notebook" }))
    fireEvent.change(await screen.findByLabelText("Page body"), {
      target: { value: "Draft" },
    })
    const patchBook = (notebook: Notebook) => {
      const state = useProjectStore.getState()
      const result = applyPatch(state, {
        revision: state.revision + 1,
        notebook,
        history: { entries: [], cursor: 0 },
        dirty: false,
      })
      expect(result.status).toBe("applied")
      useProjectStore.setState(result.state)
    }
    act(() => patchBook({ pages: [{ title: "Saved", body: "From document" }] }))
    expect(screen.getByLabelText("Page body")).toHaveValue("From document")
    act(() => patchBook({ pages: [] }))
    expect(screen.getByLabelText("Page body")).toHaveValue("")
    expect(saveButton()).toBeDisabled()
    expect(dispatch).not.toHaveBeenCalled()
  })

  it("commits the draft after a pause in typing, leaving the title being typed alone", async () => {
    vi.useFakeTimers()
    try {
      const onSave = vi.fn(async () => true)
      render(<NotebookPanel onSave={onSave} />)
      fireEvent.change(screen.getByLabelText("Page title"), {
        target: { value: "Verse " },
      })
      expect(screen.getByRole("status")).toHaveTextContent(
        "Adding your changes to the song…"
      )
      act(() => {
        vi.advanceTimersByTime(NOTEBOOK_COMMIT_DELAY_MS - 1)
      })
      fireEvent.change(screen.getByLabelText("Page body"), {
        target: { value: "Notes" },
      })
      act(() => {
        vi.advanceTimersByTime(NOTEBOOK_COMMIT_DELAY_MS - 1)
      })
      expect(onSave).not.toHaveBeenCalled()
      await act(async () => {
        vi.advanceTimersByTime(1)
      })
      expect(onSave).toHaveBeenCalledTimes(1)
      expect(onSave).toHaveBeenCalledWith({
        pages: [{ title: "Verse", body: "Notes" }],
      })
      expect(screen.getByLabelText("Page title")).toHaveValue("Verse ")
      expect(screen.getByLabelText("Page title")).toBeEnabled()
      expect(saveButton()).toBeDisabled()
      expect(screen.getByRole("status")).toHaveTextContent(
        "Notes are part of the song. Save the project to keep them."
      )
    } finally {
      vi.useRealTimers()
    }
  })

  it("commits a draft when the panel closes before the pause ends", () => {
    const onSave = vi.fn(async () => true)
    const { unmount } = render(<NotebookPanel onSave={onSave} />)
    fireEvent.change(screen.getByLabelText("Page body"), {
      target: { value: "Last words" },
    })
    expect(onSave).not.toHaveBeenCalled()
    unmount()
    expect(onSave).toHaveBeenCalledWith({
      pages: [{ title: "", body: "Last words" }],
    })
  })

  it("puts a draft into the project as one undo step when the notebook closes", async () => {
    useProjectStore.setState({
      project: { ...original.project, notebook: undefined },
    })
    vi.mocked(dispatch).mockResolvedValue(null)
    render(<NotebookControls />)
    const trigger = screen.getByRole("button", { name: "Notebook" })
    fireEvent.click(trigger)
    fireEvent.change(await screen.findByLabelText("Page body"), {
      target: { value: "Keep me" },
    })
    fireEvent.click(trigger)
    await waitFor(() =>
      expect(dispatch).toHaveBeenCalledWith(
        {
          type: "replaceNotebook",
          notebook: { pages: [{ title: "", body: "Keep me" }] },
        },
        expect.any(Number)
      )
    )
  })

  it("reads and shows a page cue as a song position", async () => {
    useProjectStore.setState({
      project: {
        ...original.project,
        settings: {
          ...original.project.settings,
          timeSignature: { numerator: 4, denominator: 4 },
        },
        playlist: { ...original.project.playlist, timeline: undefined },
      },
    })
    const onSave = vi.fn(async () => true)
    render(
      <NotebookPanel
        notebook={{
          pages: [{ title: "Verse", body: "", startTick: 3840 * 8 + 960 }],
        }}
        onSave={onSave}
      />
    )
    const cue = screen.getByLabelText("Page starts at song position")
    expect(cue).toHaveValue("9:2")
    fireEvent.change(cue, { target: { value: "5:1" } })
    fireEvent.change(cue, { target: { value: "5:9" } })
    expect(cue).toHaveAttribute("aria-invalid", "true")
    fireEvent.blur(cue)
    expect(cue).toHaveValue("5:1")
    fireEvent.click(saveButton())
    await waitFor(() =>
      expect(onSave).toHaveBeenCalledWith({
        pages: [{ title: "Verse", body: "", startTick: 3840 * 4 }],
      })
    )
    fireEvent.change(cue, { target: { value: "" } })
    expect(cue).not.toHaveAttribute("aria-invalid", "true")
    fireEvent.click(saveButton())
    await waitFor(() =>
      expect(onSave).toHaveBeenLastCalledWith({
        pages: [{ title: "Verse", body: "" }],
      })
    )
  })

  it("discards drafts when the project is replaced", async () => {
    useProjectStore.setState({
      project: { ...original.project, notebook: undefined },
    })
    render(<NotebookControls />)
    fireEvent.click(screen.getByRole("button", { name: "Notebook" }))
    fireEvent.change(await screen.findByLabelText("Page body"), {
      target: { value: "Unsaved" },
    })
    act(() => announceProjectReplaced())
    expect(screen.getByLabelText("Page body")).toHaveValue("")
    expect(dispatch).not.toHaveBeenCalled()
  })
})
