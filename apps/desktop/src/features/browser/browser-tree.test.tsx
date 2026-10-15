import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"

import type { BrowserEntry } from "@/bindings"
import { registry } from "@/lib/actions"
import type { Backend } from "@/lib/ipc"

import BrowserPanel from "."
import { refresh } from "./commands"
import { flushScrollTop, readPersisted, writePersisted } from "./persist"
import { resetPreview } from "./preview"
import { expandFolder, resetBrowserStore, useBrowserStore } from "./store"
import {
  audio,
  deferred,
  findItem,
  item,
  itemNames,
  queryItem,
  startBrowserTest,
  tree,
} from "./testing"
import { rowId } from "./tree-model"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let stop: () => void = () => undefined

async function start(override?: (mock: Backend) => Partial<Backend>) {
  const app = await startBrowserTest(override)
  stop = app.stop
  return app.backend
}

afterEach(() => stop())

const press = (key: string, init: KeyboardEventInit = {}) =>
  fireEvent.keyDown(tree(), { key, ...init })

const selectedName = () =>
  screen.queryByRole("treeitem", { selected: true })?.textContent ?? null

describe("loading folders", () => {
  it("shows the factory library open on first run and reads nothing deeper", async () => {
    const backend = await start()
    const list = vi.spyOn(backend, "browserList")
    render(<BrowserPanel />)

    expect(await findItem("Drums")).toHaveAttribute("aria-expanded", "false")
    expect(item("Factory")).toHaveAttribute("aria-expanded", "true")
    expect(itemNames()).toEqual(["Factory", "Drums", "Loops"])
    expect(list.mock.calls).toEqual([["/factory"]])
  })

  it("reads a folder when it is first opened, then keeps the listing", async () => {
    const user = userEvent.setup()
    const backend = await start()
    const list = vi.spyOn(backend, "browserList")
    render(<BrowserPanel />)

    await user.click(await findItem("Drums"))
    expect(await findItem("Kicks")).toHaveAttribute("aria-level", "3")
    expect(list).toHaveBeenCalledWith("/factory/Drums")

    await user.click(item("Drums"))
    expect(queryItem("Kicks")).not.toBeInTheDocument()
    await user.click(item("Drums"))
    expect(item("Kicks")).toBeInTheDocument()
    expect(
      list.mock.calls.filter(([path]) => path === "/factory/Drums")
    ).toHaveLength(1)
  })

  it("shows a loading line until the listing arrives", async () => {
    const user = userEvent.setup()
    const slow = deferred<BrowserEntry[]>()
    await start((mock) => ({
      browserList: (path) =>
        path === "/factory/Loops" ? slow.promise : mock.browserList(path),
    }))
    render(<BrowserPanel />)

    await user.click(await findItem("Loops"))
    expect(screen.getByText("Reading…")).toBeInTheDocument()
    expect(item("Loops")).toHaveAttribute("aria-busy", "true")

    await act(async () => {
      slow.resolve([audio("/factory/Loops", "Break.wav")])
    })
    expect(item("Break.wav")).toBeInTheDocument()
    expect(screen.queryByText("Reading…")).not.toBeInTheDocument()
  })

  it("shows why a folder could not be read, and reads it again on retry", async () => {
    const user = userEvent.setup()
    let fail = true
    const backend = await start((mock) => ({
      browserList: (path) =>
        path === "/factory/Loops" && fail
          ? Promise.reject(new Error("Access is denied."))
          : mock.browserList(path),
    }))
    const list = vi.spyOn(backend, "browserList")
    render(<BrowserPanel />)

    await user.click(await findItem("Loops"))
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not read this folder. Access is denied."
    )
    // The rest of the tree is untouched by one bad folder.
    expect(item("Drums")).toBeInTheDocument()

    fail = false
    await user.click(screen.getByRole("button", { name: "Retry" }))
    expect(await findItem("Drum loop 128.wav")).toBeInTheDocument()
    expect(
      list.mock.calls.filter(([path]) => path === "/factory/Loops")
    ).toHaveLength(2)
  })

  it("reads a failed folder again when it is reopened", async () => {
    let fail = true
    await start((mock) => ({
      browserList: (path) =>
        path === "/factory/Loops" && fail
          ? Promise.reject(new Error("Access is denied."))
          : mock.browserList(path),
    }))
    render(<BrowserPanel />)
    await findItem("Loops")

    press("End")
    press("Enter")
    expect(await screen.findByText(/Access is denied/)).toBeInTheDocument()
    expect(item("Loops")).toHaveAttribute("aria-expanded", "true")

    // Closing and opening it again is a retry.
    fail = false
    press("Enter")
    expect(item("Loops")).toHaveAttribute("aria-expanded", "false")
    press("Enter")
    expect(await findItem("Drum loop 128.wav")).toBeInTheDocument()
  })

  it("says so when a folder is empty", async () => {
    const user = userEvent.setup()
    await start((mock) => ({
      browserList: (path) =>
        path === "/factory/Loops"
          ? Promise.resolve([])
          : mock.browserList(path),
    }))
    render(<BrowserPanel />)

    await user.click(await findItem("Loops"))
    expect(await screen.findByText("Empty folder")).toBeInTheDocument()
  })

  it("reads a folder again on refresh and shows what changed", async () => {
    let extra = false
    const backend = await start((mock) => ({
      browserList: async (path) => {
        const entries = await mock.browserList(path)
        return path === "/factory" && extra
          ? [...entries, audio("/factory", "New.wav")]
          : entries
      },
    }))
    const list = vi.spyOn(backend, "browserList")
    render(<BrowserPanel />)
    await findItem("Drums")

    extra = true
    fireEvent.contextMenu(item("Factory"))
    fireEvent.click(await screen.findByRole("menuitem", { name: "Refresh" }))

    expect(await findItem("New.wav")).toBeInTheDocument()
    expect(
      list.mock.calls.filter(([path]) => path === "/factory")
    ).toHaveLength(2)
  })

  it("keeps only the newest answer when a folder is read twice at once", async () => {
    const answers: ReturnType<typeof deferred<BrowserEntry[]>>[] = []
    let blocking = false
    await start((mock) => ({
      browserList: (path) => {
        if (path !== "/factory" || !blocking) return mock.browserList(path)
        const answer = deferred<BrowserEntry[]>()
        answers.push(answer)
        return answer.promise
      },
    }))
    render(<BrowserPanel />)
    await findItem("Drums")

    blocking = true
    act(() => refresh("/factory"))
    act(() => refresh("/factory"))
    expect(answers).toHaveLength(2)

    // The second read answers first, then the first one limps in.
    await act(async () => {
      answers[1].resolve([audio("/factory", "Second.wav")])
    })
    await act(async () => {
      answers[0].resolve([audio("/factory", "First.wav")])
    })
    expect(item("Second.wav")).toBeInTheDocument()
    expect(queryItem("First.wav")).not.toBeInTheDocument()
  })

  it("offers to try again when the browser itself cannot be read", async () => {
    const user = userEvent.setup()
    let fail = true
    await start((mock) => ({
      browserRoots: () =>
        fail ? Promise.reject(new Error("No library")) : mock.browserRoots(),
    }))
    render(<BrowserPanel />)

    expect(
      await screen.findByText("The browser could not be read.")
    ).toBeInTheDocument()
    expect(screen.getByText("No library")).toBeInTheDocument()

    fail = false
    await user.click(screen.getByRole("button", { name: "Try again" }))
    expect(await findItem("Factory")).toBeInTheDocument()
  })

  it("draws only the rows on screen of a 10,000-file folder", async () => {
    const files = Array.from({ length: 10_000 }, (_, index) =>
      audio("/factory", `Sound ${String(index).padStart(5, "0")}.wav`)
    )
    await start(() => ({ browserList: () => Promise.resolve(files) }))
    render(<BrowserPanel />)

    expect(await findItem("Sound 00000.wav")).toHaveAttribute(
      "aria-setsize",
      "10000"
    )
    expect(screen.getAllByRole("treeitem").length).toBeLessThan(60)
    expect(tree().style.height).toBe(`${10_001 * 22}px`)

    press("End")
    expect(selectedName()).toBe("Sound 09999.wav")
    expect(item("Sound 09999.wav")).toHaveAttribute("aria-posinset", "10000")
  })
})

describe("keyboard", () => {
  async function openDrums() {
    const user = userEvent.setup()
    const backend = await start()
    render(<BrowserPanel />)
    await user.click(await findItem("Drums"))
    await findItem("Kicks")
    return { user, backend }
  }

  it("has tree semantics and points at the selected row", async () => {
    await openDrums()
    expect(tree()).toHaveAttribute("aria-label", "Sounds and folders")
    expect(tree()).toHaveAttribute("tabindex", "0")
    expect(tree()).toHaveAttribute("aria-activedescendant", item("Drums").id)
    expect(item("Drums")).toHaveAttribute("aria-selected", "true")
    expect(item("Drums")).toHaveAttribute("aria-expanded", "true")
    expect(item("Kicks")).toHaveAttribute("aria-level", "3")
    // Folders come sorted by name: Claps, Hats, Kicks, Percussion, Snares.
    expect(item("Kicks")).toHaveAttribute("aria-posinset", "3")
    expect(item("Kicks")).toHaveAttribute("aria-setsize", "5")
  })

  it("moves with the arrow keys, Home and End", async () => {
    await openDrums()
    press("Home")
    expect(selectedName()).toBe("Factory")
    press("ArrowDown")
    press("ArrowDown")
    expect(selectedName()).toBe("Claps")
    press("ArrowUp")
    expect(selectedName()).toBe("Drums")
    press("End")
    expect(selectedName()).toBe("Loops")
    press("ArrowDown")
    expect(selectedName()).toBe("Loops")
  })

  it("starts at the first row when nothing is selected", async () => {
    await start()
    render(<BrowserPanel />)
    await findItem("Drums")
    expect(selectedName()).toBeNull()
    press("ArrowDown")
    expect(selectedName()).toBe("Factory")
  })

  it("opens with Right, steps in with Right again, and backs out with Left", async () => {
    await openDrums()
    press("k")
    expect(selectedName()).toBe("Kicks")

    press("ArrowRight")
    expect(await findItem("Kick 01.wav")).toBeInTheDocument()
    expect(selectedName()).toBe("Kicks")
    press("ArrowRight")
    expect(selectedName()).toBe("Kick 01.wav")
    press("ArrowRight")
    expect(selectedName()).toBe("Kick 01.wav")

    press("ArrowLeft")
    expect(selectedName()).toBe("Kicks")
    press("ArrowLeft")
    expect(item("Kicks")).toHaveAttribute("aria-expanded", "false")
    expect(queryItem("Kick 01.wav")).not.toBeInTheDocument()
    press("ArrowLeft")
    expect(selectedName()).toBe("Drums")
  })

  it("toggles a folder with Enter", async () => {
    await openDrums()
    press("Enter")
    expect(item("Drums")).toHaveAttribute("aria-expanded", "false")
    press("Enter")
    expect(item("Drums")).toHaveAttribute("aria-expanded", "true")
  })

  it("jumps to rows by typing their first letters", async () => {
    await openDrums()
    press("l")
    expect(selectedName()).toBe("Loops")
    // Still within the same burst: "lo" stays on Loops.
    press("o")
    expect(selectedName()).toBe("Loops")
  })

  it("cycles through rows that share a first letter", async () => {
    const { user } = await openDrums()
    await user.click(item("Kicks"))
    await findItem("Kick 01.wav")

    // Each press is a new search, because the presses are far apart.
    const now = vi.spyOn(performance, "now")
    const names: (string | null)[] = []
    for (const time of [10_000, 20_000, 30_000, 40_000, 50_000]) {
      now.mockReturnValue(time)
      press("k")
      names.push(selectedName())
    }
    now.mockRestore()
    expect(names).toEqual([
      "Kick 01.wav",
      "Kick 02.wav",
      "Kick 03.wav",
      "Kick Punch.wav",
      "Kicks",
    ])
  })

  it("leaves Space and shortcuts with Ctrl or Alt to the app", async () => {
    const { backend } = await openDrums()
    const toggle = vi.spyOn(backend, "transportToggle")

    // Space is play and stop everywhere, the tree included.
    press(" ", { code: "Space" })
    await waitFor(() => expect(toggle).toHaveBeenCalledTimes(1))
    press("ArrowDown", { ctrlKey: true })
    press("ArrowDown", { altKey: true })
    press("k", { ctrlKey: true })
    expect(selectedName()).toBe("Drums")

    // fireEvent returns false when a handler called preventDefault.
    expect(press("ArrowDown")).toBe(false)
    expect(selectedName()).toBe("Claps")
  })

  it("keeps Delete from another panel's action, and leaves the key alone", async () => {
    const deleteChannel = vi.fn()
    const remove = registry.register([
      {
        id: "test.deleteChannel",
        title: "Delete channel",
        section: "Test",
        scope: "channelRack",
        defaultShortcut: ["Delete", "Backspace"],
        run: deleteChannel,
      },
    ])
    await openDrums()
    act(() => tree().focus())
    // Nothing in the browser uses the keys, so they are not cancelled.
    expect(press("Delete", { code: "Delete" })).toBe(true)
    expect(press("Backspace", { code: "Backspace" })).toBe(true)
    expect(deleteChannel).not.toHaveBeenCalled()
    remove()
  })

  it("skips files Windfall cannot use, and shows them as disabled", async () => {
    const user = userEvent.setup()
    await start((mock) => ({
      browserRoots: async () => [
        ...(await mock.browserRoots()),
        { name: "Mine", path: "/mine", kind: "user" },
      ],
      browserList: async (path) =>
        path === "/mine"
          ? [
              { name: "Notes.txt", path: "/mine/Notes.txt", kind: "other" },
              audio("/mine", "Zap.wav"),
            ]
          : mock.browserList(path),
    }))
    render(<BrowserPanel />)

    await findItem("Mine")
    act(() => expandFolder(rowId("/mine", "/mine"), "/mine"))
    expect(await findItem("Notes.txt")).toHaveAttribute("aria-disabled", "true")
    await user.click(item("Zap.wav"))
    expect(selectedName()).toBe("Zap.wav")
    press("ArrowUp")
    expect(selectedName()).toBe("Mine")
    press("ArrowDown")
    expect(selectedName()).toBe("Zap.wav")
    await user.click(item("Notes.txt"))
    expect(selectedName()).toBe("Zap.wav")
  })

  it("leaves the factory library's support files out", async () => {
    await start((mock) => ({
      browserList: async (path) =>
        path === "/factory"
          ? [
              ...(await mock.browserList(path)),
              { name: "LICENSE.md", path: "/factory/LICENSE.md", kind: "other" },
              { name: "README.md", path: "/factory/README.md", kind: "other" },
            ]
          : mock.browserList(path),
    }))
    render(<BrowserPanel />)

    expect(await findItem("Loops")).toBeInTheDocument()
    expect(itemNames()).toEqual(["Factory", "Drums", "Loops"])
    expect(queryItem("LICENSE.md")).not.toBeInTheDocument()
  })
})

describe("filter", () => {
  async function openKicks() {
    const user = userEvent.setup()
    await start()
    render(<BrowserPanel />)
    await user.click(await findItem("Drums"))
    await user.click(await findItem("Kicks"))
    await findItem("Kick 01.wav")
    return user
  }

  const box = () =>
    screen.getByRole("searchbox", { name: "Filter the browser" })

  it("searches recursive paths with case, wildcards and Boolean operators", async () => {
    const user = await openKicks()
    await user.type(box(), 'DRUMS/K?CKS AND "KICK 0" NOT 03')
    await findItem("Kick 02.wav")
    expect(
      screen
        .getAllByRole("treeitem")
        .map((r) => r.getAttribute("aria-description"))
    ).toEqual(["Drums/Kicks/Kick 01.wav", "Drums/Kicks/Kick 02.wav"])
    expect(item("Kick 02.wav")).toHaveAttribute("aria-level", "1")
  })

  it("finds files in closed folders without reading the tree", async () => {
    const user = userEvent.setup()
    const backend = await start()
    const list = vi.spyOn(backend, "browserList")
    render(<BrowserPanel />)
    await findItem("Drums")
    await user.type(box(), '"snare 01"')
    expect(await findItem("Snare 01.wav")).toBeInTheDocument()
    expect(list.mock.calls).toEqual([["/factory"]])
    expect(screen.getAllByRole("treeitem")).toHaveLength(1)
  })

  it("searches every user folder before it is opened", async () => {
    const user = userEvent.setup()
    const backend = await start()
    await backend.browserAddRoot("/samples/Mine")
    const list = vi.spyOn(backend, "browserList")
    render(<BrowserPanel />)
    await findItem("Drums")
    await user.type(box(), '"vocal chop" OR snare*')
    expect(await findItem("Vocal chop.wav")).toBeInTheDocument()
    expect(await findItem("Snare Tight.wav")).toBeInTheDocument()
    expect(list.mock.calls).toEqual([["/factory"]])
    expect(screen.getByRole("note")).toHaveTextContent(
      "requires the desktop app"
    )
  })

  it("explains an empty result and clears every filter", async () => {
    const user = await openKicks()
    await user.type(box(), "zither")
    expect(
      await screen.findByText("No library files match these filters.")
    ).toBeInTheDocument()
    expect(screen.queryAllByRole("treeitem")).toHaveLength(0)
    await user.click(screen.getByRole("button", { name: "Show everything" }))
    expect(box()).toHaveValue("")
    expect(box()).toHaveFocus()
    expect(item("Kicks")).toBeInTheDocument()
  })

  it("clears with its button and with Escape", async () => {
    const user = await openKicks()
    await user.type(box(), "kick")
    await user.click(screen.getByRole("button", { name: "Clear the filter" }))
    expect(box()).toHaveValue("")
    expect(item("Snares")).toBeInTheDocument()

    await user.type(box(), "kick")
    await user.keyboard("{Escape}")
    expect(box()).toHaveValue("")
  })

  it("takes the focus on Ctrl+F and hands it to the tree on Arrow Down", async () => {
    const user = await openKicks()
    expect(box()).not.toHaveFocus()
    await user.keyboard("{Control>}f{/Control}")
    expect(box()).toHaveFocus()

    await user.keyboard("kick{ArrowDown}")
    expect(tree()).toHaveFocus()
  })
})

describe("remembering the layout", () => {
  /** Unmounts, forgets everything in memory and starts again from storage. */
  function restart(unmount: () => void) {
    unmount()
    flushScrollTop()
    resetBrowserStore()
    resetPreview()
    return render(<BrowserPanel />)
  }

  it("brings back the open folders after a restart", async () => {
    const user = userEvent.setup()
    const backend = await start()
    const first = render(<BrowserPanel />)
    await user.click(await findItem("Drums"))
    await user.click(await findItem("Hats"))
    await findItem("Open Hat 01.wav")

    const list = vi.spyOn(backend, "browserList")
    restart(first.unmount)

    expect(await findItem("Open Hat 01.wav")).toBeInTheDocument()
    expect(item("Drums")).toHaveAttribute("aria-expanded", "true")
    expect(item("Kicks")).toHaveAttribute("aria-expanded", "false")
    expect(list.mock.calls.map(([path]) => path).sort()).toEqual([
      "/factory",
      "/factory/Drums",
      "/factory/Drums/Hats",
    ])
  })

  it("remembers that the factory library was closed", async () => {
    const user = userEvent.setup()
    await start()
    const first = render(<BrowserPanel />)
    await findItem("Drums")
    await user.click(item("Factory"))
    expect(queryItem("Drums")).not.toBeInTheDocument()

    restart(first.unmount)
    expect(await findItem("Factory")).toHaveAttribute("aria-expanded", "false")
    expect(queryItem("Drums")).not.toBeInTheDocument()
  })

  it("keeps everything while the panel is only hidden", async () => {
    const user = userEvent.setup()
    const backend = await start()
    const first = render(<BrowserPanel />)
    await user.click(await findItem("Drums"))
    await user.click(await findItem("Kicks"))
    await user.click(await findItem("Kick 02.wav"))

    const list = vi.spyOn(backend, "browserList")
    first.unmount()
    render(<BrowserPanel />)

    // No reading: the listings and the selection never left memory.
    expect(item("Kick 02.wav")).toHaveAttribute("aria-selected", "true")
    expect(list).not.toHaveBeenCalled()
  })

  it("quietly drops folders that are gone", async () => {
    const { toast } = await import("sonner")
    writePersisted({
      expanded: [
        rowId("/factory", "/factory"),
        rowId("/factory", "/factory/Drums"),
        rowId("/factory", "/factory/Deleted"),
        rowId("/factory", "/factory/Deleted/Deeper"),
        rowId("/gone", "/gone"),
        rowId("/gone", "/gone/Sub"),
        "nonsense",
      ],
    })
    await start()
    render(<BrowserPanel />)

    await findItem("Kicks")
    await waitFor(() =>
      expect(useBrowserStore.getState().restoring).toBe(false)
    )
    expect([...(readPersisted().expanded ?? [])].sort()).toEqual(
      [
        rowId("/factory", "/factory"),
        rowId("/factory", "/factory/Drums"),
      ].sort()
    )
    expect(toast.error).not.toHaveBeenCalled()
  })

  it("puts the list back where it was scrolled to", async () => {
    const user = userEvent.setup()
    await start()
    const first = render(<BrowserPanel />)
    await user.click(await findItem("Drums"))
    await findItem("Kicks")

    const scroller = () =>
      document.querySelector<HTMLElement>("[data-slot=browser-scroll]")!
    scroller().scrollTop = 66
    fireEvent.scroll(scroller())

    restart(first.unmount)
    await findItem("Kicks")
    await waitFor(() =>
      expect(useBrowserStore.getState().restoring).toBe(false)
    )
    expect(scroller().scrollTop).toBe(66)
    expect(readPersisted().scrollTop).toBe(66)
  })
})
