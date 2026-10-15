import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"

import type { LibraryResults } from "@/bindings"
import { useProjectStore } from "@/lib/store/project"

import BrowserPanel from "."
import { addToRack, removeRoot } from "./commands"
import { useLibraryStore } from "./library-store"
import { setFilter } from "./store"
import { deferred, findItem, item, startBrowserTest, tree } from "./testing"

let stop = () => {}
afterEach(() => stop())
const query = () =>
  screen.getByRole("searchbox", { name: "Filter the browser" })

describe("browser library workflow", () => {
  it("edits favorites/tags, filters them and retains selection, audition and one-step import", async () => {
    const app = await startBrowserTest()
    stop = app.stop
    const user = userEvent.setup()
    const preview = vi.spyOn(app.backend, "previewPlay")
    render(<BrowserPanel />)
    await findItem("Drums")
    await user.type(query(), '"kick 02"')
    await user.click(await findItem("Kick 02.wav"))
    await waitFor(() =>
      expect(preview).toHaveBeenCalledWith(
        "/factory/Drums/Kicks/Kick 02.wav",
        expect.objectContaining({
          rootPath: "/factory",
          fingerprint: "fixture-v1",
        })
      )
    )
    const favorite = await screen.findByRole("button", {
      name: "Star selected file",
    })
    await waitFor(() => expect(favorite).toBeEnabled())
    // The tag form is closed until asked for, so the tree keeps its room.
    expect(screen.queryByRole("textbox", { name: "Tags" })).toBeNull()
    await user.click(screen.getByRole("button", { name: "Edit tags" }))
    await user.type(
      screen.getByRole("textbox", { name: "Tags" }),
      "Warm, DRUM, warm"
    )
    await user.click(screen.getByRole("button", { name: "Save tags" }))
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Tags" })).toHaveValue(
        "drum, warm"
      )
    )
    await user.click(favorite)
    await waitFor(() =>
      expect(favorite).toHaveAttribute("aria-pressed", "true")
    )
    await user.clear(query())
    await user.click(screen.getByRole("button", { name: "Favorites only" }))
    await findItem("Kick 02.wav")
    expect(screen.getAllByRole("treeitem")).toHaveLength(1)
    await user.click(screen.getByRole("combobox", { name: "Filter by tag" }))
    await user.click(await screen.findByRole("option", { name: "warm" }))
    await findItem("Kick 02.wav")
    const before = useProjectStore.getState().project.channels.length
    fireEvent.keyDown(tree(), { key: "Enter" })
    await waitFor(() =>
      expect(useProjectStore.getState().project.channels).toHaveLength(
        before + 1
      )
    )
    expect(useProjectStore.getState().history.entries.at(-1)?.label).toBe(
      "Add channel"
    )
    await app.backend.undo()
    await waitFor(() =>
      expect(useProjectStore.getState().project.channels).toHaveLength(before)
    )
  })

  it("shows loading, partial limits and folder errors while tree audition/import stays usable", async () => {
    const app = await startBrowserTest((mock) => ({
      librarySearch: async (search) => ({
        ...(await mock.librarySearch(search)),
        status: "indexing",
        truncated: true,
        resultsTruncated: true,
        issues: ["Could not read Offline. Reconnect the drive, then refresh."],
      }),
    }))
    stop = app.stop
    const user = userEvent.setup()
    render(<BrowserPanel />)
    await findItem("Drums")
    expect(await screen.findByText(/Indexing .* files/)).toBeInTheDocument()
    expect(screen.getByText(/Index limit reached/)).toBeInTheDocument()
    expect(screen.getByText(/Showing the first 500/)).toBeInTheDocument()
    expect(screen.getByRole("alert")).toHaveTextContent("Reconnect")
    await user.click(item("Drums"))
    await user.click(await findItem("Kicks"))
    await user.click(await findItem("Kick 02.wav"))
    expect(await screen.findByText("0.55 s")).toBeInTheDocument()
    const before = useProjectStore.getState().project.channels.length
    await user.click(screen.getByRole("button", { name: "Add to rack" }))
    await waitFor(() =>
      expect(useProjectStore.getState().project.channels).toHaveLength(
        before + 1
      )
    )
    const cancel = vi.spyOn(app.backend, "libraryCancel")
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    expect(cancel).toHaveBeenCalledWith(expect.any(Number))
  })

  it("ignores late answers for old queries and removed roots; old captured imports fail", async () => {
    const first = deferred<LibraryResults>()
    const app = await startBrowserTest((mock) => ({
      librarySearch: (search) =>
        search.query === "first" ? first.promise : mock.librarySearch(search),
    }))
    stop = app.stop
    const user = userEvent.setup()
    const roots = await app.backend.browserAddRoot("/samples/Mine")
    const stale = (
      await app.backend.librarySearch({
        query: "chop",
        favoritesOnly: false,
        tags: [],
      })
    ).entries[0]
    render(<BrowserPanel />)
    await findItem("Drums")
    await act(async () => setFilter("first"))
    await waitFor(() => expect(useLibraryStore.getState().pending).toBe(true))
    // Allow the debounced first request to start before changing the query.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 170))
    })
    await act(async () => setFilter("chop"))
    await user.click(await findItem("Vocal chop.wav"))
    const before = useProjectStore.getState().project.channels.length
    await act(async () => removeRoot(roots.find((r) => r.kind === "user")!))
    await waitFor(() =>
      expect(
        screen.queryByRole("treeitem", { name: "Vocal chop.wav" })
      ).toBeNull()
    )
    await act(async () => addToRack(stale.entry.path, stale.token))
    expect(useProjectStore.getState().project.channels).toHaveLength(before)
    const old = await app.backend.librarySearch({
      query: "kick",
      favoritesOnly: false,
      tags: [],
    })
    await act(async () => first.resolve(old))
    expect(screen.queryByRole("treeitem", { name: "Kick 01.wav" })).toBeNull()
  })

  it("reports malformed Boolean queries without preventing normal folder browsing", async () => {
    const app = await startBrowserTest()
    stop = app.stop
    const user = userEvent.setup()
    render(<BrowserPanel />)
    await findItem("Drums")
    await user.type(query(), "kick OR")
    expect(await screen.findByRole("alert")).toHaveTextContent("term")
    await user.clear(query())
    await user.click(item("Drums"))
    expect(await findItem("Kicks")).toBeInTheDocument()
  })
})
