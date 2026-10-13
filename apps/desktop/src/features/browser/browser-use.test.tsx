import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, describe, expect, it, vi } from "vitest"

import type { BrowserEntry } from "@/bindings"
import { registry } from "@/lib/actions"
import { sourceSample } from "@/lib/channel-source"
import { SAMPLE_DRAG_TYPE } from "@/lib/dnd"
import type { Backend } from "@/lib/ipc"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { usePromptStore } from "@/lib/store/prompts"
import { usePlaylistStore } from "@/features/playlist/store"
import { useUiStore } from "@/lib/store/ui"

import BrowserPanel from "."
import { BROWSER_ACTIONS } from "./actions"
import { useBrowserStore } from "./store"
import {
  findItem,
  item,
  itemNames,
  queryItem,
  startBrowserTest,
  tree,
} from "./testing"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let stop: () => void = () => undefined
afterEach(() => {
  stop()
  vi.clearAllMocks()
})

const KICK = "/factory/Drums/Kicks/Kick 02.wav"

async function start(override?: (mock: Backend) => Partial<Backend>) {
  const user = userEvent.setup()
  const app = await startBrowserTest(override)
  stop = app.stop
  render(<BrowserPanel />)
  await findItem("Drums")
  return { user, backend: app.backend }
}

async function openKicks(override?: (mock: Backend) => Partial<Backend>) {
  const app = await start(override)
  await app.user.click(item("Drums"))
  await app.user.click(await findItem("Kicks"))
  await findItem("Kick 02.wav")
  return app
}

const channels = () => useProjectStore.getState().project.channels

async function openMenu(name: string) {
  fireEvent.contextMenu(item(name))
  await screen.findByRole("menu")
  return screen.getAllByRole("menuitem").map((entry) => entry.textContent)
}

const menuItem = (name: string) => screen.getByRole("menuitem", { name })

describe("adding a sound to the rack", () => {
  it("adds a channel on double-click and selects it in the rack", async () => {
    const { user, backend } = await openKicks()
    const add = vi.spyOn(backend, "addChannelFromFile")
    const before = channels().length
    useUiStore.getState().showCenterTab("playlist")

    await user.dblClick(item("Kick 02.wav"))
    await waitFor(() => expect(channels()).toHaveLength(before + 1))

    expect(add.mock.calls.map(([path]) => [path])).toEqual([[KICK]])
    const added = channels()[before]
    expect(added.name).toBe("Kick 02")
    expect(useUiStore.getState().selectedChannel).toBe(added.id)
    expect(useUiStore.getState().centerTab).toBe("channelRack")
  })

  it("adds a channel with Enter, once however long the key is held", async () => {
    const { user, backend } = await openKicks()
    const add = vi.spyOn(backend, "addChannelFromFile")
    await user.click(item("Kick 02.wav"))

    fireEvent.keyDown(tree(), { key: "Enter" })
    fireEvent.keyDown(tree(), { key: "Enter", repeat: true })
    fireEvent.keyDown(tree(), { key: "Enter", repeat: true })
    await waitFor(() => expect(add).toHaveBeenCalledTimes(1))
    expect(add).toHaveBeenCalledWith(
      KICK,
      undefined,
      expect.objectContaining({ path: KICK })
    )
  })

  it("adds a channel from the pane and from the right-click menu", async () => {
    const { user, backend } = await openKicks()
    const add = vi.spyOn(backend, "addChannelFromFile")

    expect(await openMenu("Kick 01.wav")).toEqual([
      "Preview",
      "Add to new channelEnter",
      "Add to playlist",
      "Separate into four stems",
      "Replace selected channel's sample",
      "Copy path",
    ])
    fireEvent.click(
      screen.getByRole("menuitem", { name: /^Add to new channel/ })
    )
    await waitFor(() => expect(add).toHaveBeenCalledTimes(1))
    expect(add).toHaveBeenLastCalledWith(
      "/factory/Drums/Kicks/Kick 01.wav",
      undefined,
      expect.objectContaining({ rootPath: "/factory" })
    )
    await waitFor(() =>
      expect(screen.queryByRole("menu")).not.toBeInTheDocument()
    )

    await user.click(item("Kick 02.wav"))
    await user.click(screen.getByRole("button", { name: "Add to rack" }))
    await waitFor(() => expect(add).toHaveBeenCalledTimes(2))
    expect(add).toHaveBeenLastCalledWith(
      KICK,
      undefined,
      expect.objectContaining({ path: KICK })
    )
  })

  it("shows a failure and adds nothing", async () => {
    const { user } = await openKicks(() => ({
      addChannelFromFile: () => Promise.reject(new Error("Disk error")),
    }))
    const before = channels().length
    await user.dblClick(item("Kick 02.wav"))
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Could not add the sound", {
        description: "Disk error",
      })
    )
    expect(channels()).toHaveLength(before)
  })

  it("previews from the right-click menu without adding anything", async () => {
    const { backend } = await openKicks()
    const play = vi.spyOn(backend, "previewPlay")
    const add = vi.spyOn(backend, "addChannelFromFile")

    await openMenu("Kick 03.wav")
    // The right-click selected the row but stayed silent.
    expect(useBrowserStore.getState().selected?.name).toBe("Kick 03.wav")
    expect(play).not.toHaveBeenCalled()

    fireEvent.click(menuItem("Preview"))
    expect(play.mock.calls.map(([path]) => [path])).toEqual([
      ["/factory/Drums/Kicks/Kick 03.wav"],
    ])
    expect(add).not.toHaveBeenCalled()
  })
})

describe("adding a sound to the playlist", () => {
  const clips = () => useProjectStore.getState().project.playlist.clips

  it("adds an audio clip from the right-click menu and shows the playlist", async () => {
    const { backend } = await openKicks()
    const add = vi.spyOn(backend, "addAudioClipFromFile")
    useUiStore.getState().showCenterTab("channelRack")

    await openMenu("Kick 02.wav")
    fireEvent.click(menuItem("Add to playlist"))
    await waitFor(() => expect(clips()).toHaveLength(1))

    // At the song position, on a new track of its own.
    expect(add.mock.calls.map(([path, place]) => [path, place])).toEqual([
      [KICK, { track: undefined, start: 0, mixerTrack: undefined }],
    ])
    expect(clips()[0].content).toMatchObject({ type: "audio" })
    const { history, project } = useProjectStore.getState()
    expect(history.entries.map((entry) => entry.label)).toEqual([
      "Add audio clip",
    ])
    expect(project.playlist.tracks).toHaveLength(1)
    expect(useUiStore.getState().centerTab).toBe("playlist")
    expect([...usePlaylistStore.getState().selection]).toEqual([clips()[0].id])
  })

  it("adds one from the button under the waveform", async () => {
    const { user } = await openKicks()
    await user.click(item("Kick 02.wav"))
    await user.click(
      await screen.findByRole("button", { name: "Add to playlist" })
    )
    await waitFor(() => expect(clips()).toHaveLength(1))
    expect(clips()[0]).toMatchObject({ start: 0 })
  })
})

describe("replacing the selected channel's sample", () => {
  it("is off until a channel is selected, then names that channel", async () => {
    const { user, backend } = await openKicks()
    const replace = vi.spyOn(backend, "setChannelSampleFromFile")
    await user.click(item("Kick 02.wav"))

    const off = screen.getByRole("button", {
      name: "Replace selected channel's sample",
    })
    expect(off).toBeDisabled()

    const snare = channels().find((channel) => channel.name === "Snare")!
    useUiStore.getState().selectChannel(snare.id)

    const on = await screen.findByRole("button", {
      name: "Replace the sample of Snare",
    })
    expect(on).toBeEnabled()
    expect(on).toHaveTextContent("Replace Snare")
    await user.click(on)

    await waitFor(() =>
      expect(
        replace.mock.calls.map(([channel, path]) => [channel, path])
      ).toEqual([[snare.id, KICK]])
    )
    const sampleId = sourceSample(
      channels().find((channel) => channel.id === snare.id)?.source
    )
    const sample = useProjectStore
      .getState()
      .project.samples.find((asset) => asset.id === sampleId)
    expect(sample?.name).toBe("Kick 02")
  })

  it("is offered in the right-click menu with the channel's name", async () => {
    const { backend } = await openKicks()
    const replace = vi.spyOn(backend, "setChannelSampleFromFile")
    const hat = channels().find((channel) => channel.name === "Hat")!
    useUiStore.getState().selectChannel(hat.id)

    await openMenu("Kick 02.wav")
    fireEvent.click(menuItem("Replace sample of Hat"))
    await waitFor(() =>
      expect(
        replace.mock.calls.map(([channel, path]) => [channel, path])
      ).toEqual([[hat.id, KICK]])
    )
  })

  it("goes off again when the selected channel is deleted", async () => {
    const { user } = await openKicks()
    const clap = channels().find((channel) => channel.name === "Clap")!
    useUiStore.getState().selectChannel(clap.id)
    await user.click(item("Kick 02.wav"))
    await screen.findByRole("button", { name: "Replace the sample of Clap" })

    await dispatch({ type: "removeChannel", id: clap.id })
    expect(
      await screen.findByRole("button", {
        name: "Replace selected channel's sample",
      })
    ).toBeDisabled()
  })
})

describe("project files", () => {
  const SONG = "/factory/Song.windfall"

  function withProjectFile(mock: Backend): Partial<Backend> {
    return {
      browserList: async (path) => {
        const entries = await mock.browserList(path)
        const song: BrowserEntry = {
          name: "Song.windfall",
          path: SONG,
          kind: "project",
        }
        return path === "/factory" ? [...entries, song] : entries
      },
    }
  }

  it("opens on double-click when there is nothing to lose", async () => {
    const { user, backend } = await start(withProjectFile)
    // The mock only opens projects it has saved, so save one at this path.
    await backend.projectSave(SONG)
    useProjectStore.setState({ dirty: false })
    const open = vi.spyOn(backend, "projectOpen")

    await user.dblClick(item("Song.windfall"))
    await waitFor(() => expect(open.mock.calls).toEqual([[SONG]]))
    await waitFor(() => expect(useProjectStore.getState().path).toBe(SONG))
    expect(usePromptStore.getState().confirm).toBeNull()
  })

  it("asks before throwing away unsaved changes", async () => {
    const { user, backend } = await start(withProjectFile)
    await backend.projectSave(SONG)
    const open = vi.spyOn(backend, "projectOpen")
    await dispatch({ type: "addChannel" })
    expect(useProjectStore.getState().dirty).toBe(true)

    await user.click(item("Song.windfall"))
    fireEvent.keyDown(tree(), { key: "Enter" })
    await waitFor(() =>
      expect(usePromptStore.getState().confirm).not.toBeNull()
    )
    expect(open).not.toHaveBeenCalled()

    // Cancelling leaves the project alone.
    usePromptStore.getState().confirm?.resolve(null)
    await waitFor(() => expect(usePromptStore.getState().confirm).toBeNull())
    expect(open).not.toHaveBeenCalled()

    await user.click(screen.getByRole("button", { name: "Open project" }))
    await waitFor(() =>
      expect(usePromptStore.getState().confirm).not.toBeNull()
    )
    usePromptStore.getState().confirm?.resolve("discard")
    await waitFor(() => expect(open.mock.calls).toEqual([[SONG]]))
  })

  it("does not play or add a project file", async () => {
    const { user, backend } = await start(withProjectFile)
    const play = vi.spyOn(backend, "previewPlay")
    const add = vi.spyOn(backend, "addChannelFromFile")
    await user.click(item("Song.windfall"))

    expect(play).not.toHaveBeenCalled()
    expect(add).not.toHaveBeenCalled()
    expect(await openMenu("Song.windfall")).toEqual([
      "Open projectEnter",
      "Copy path",
    ])
  })
})

describe("user folders", () => {
  it("prompts for a first folder, adds it open and selected, and drops the prompt", async () => {
    const { user, backend } = await start()
    const addRoot = vi.spyOn(backend, "browserAddRoot")
    expect(screen.getByText(/Your own samples go here too/)).toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Add folder…" }))

    // The test dialogs pick "/samples/Test".
    expect(await findItem("Recording 01.wav")).toBeInTheDocument()
    expect(addRoot.mock.calls).toEqual([["/samples/Test"]])
    expect(item("Test")).toHaveAttribute("aria-expanded", "true")
    expect(item("Test")).toHaveAttribute("aria-selected", "true")
    expect(itemNames().slice(0, 4)).toEqual([
      "Factory",
      "Drums",
      "Loops",
      "Test",
    ])
    expect(
      screen.queryByText(/Your own samples go here too/)
    ).not.toBeInTheDocument()
  })

  it("adds a folder from the header button", async () => {
    const { user } = await start()
    await user.click(
      screen.getByRole("button", { name: "Add folder to the browser" })
    )
    expect(await findItem("Test")).toBeInTheDocument()
  })

  it("does nothing when the folder dialog is cancelled", async () => {
    const { user, backend } = await start(() => ({
      pickFolder: () => Promise.resolve(null),
    }))
    const addRoot = vi.spyOn(backend, "browserAddRoot")
    await user.click(screen.getByRole("button", { name: "Add folder…" }))
    expect(addRoot).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
  })

  it("shows why a folder could not be added", async () => {
    const { user } = await start()
    await user.click(screen.getByRole("button", { name: "Add folder…" }))
    await findItem("Test")

    // The same folder again: the backend refuses.
    await user.click(
      screen.getByRole("button", { name: "Add folder to the browser" })
    )
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Could not add the folder", {
        description: '"/samples/Test" is already in the browser.',
      })
    )
    expect(screen.getAllByRole("treeitem", { name: "Test" })).toHaveLength(1)
  })

  it("removes a user folder from its menu, and never offers that for Factory", async () => {
    const { user, backend } = await start()
    const removeRoot = vi.spyOn(backend, "browserRemoveRoot")
    await user.click(screen.getByRole("button", { name: "Add folder…" }))
    await findItem("Recording 01.wav")

    expect(await openMenu("Factory")).toEqual([
      "Collapse",
      "Refresh",
      "Copy path",
      "Add folder to the browser…",
    ])
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" })
    await waitFor(() =>
      expect(screen.queryByRole("menu")).not.toBeInTheDocument()
    )

    expect(await openMenu("Test")).toEqual([
      "Collapse",
      "Refresh",
      "Copy path",
      "Add folder to the browser…",
      "Remove from browser",
    ])
    fireEvent.click(menuItem("Remove from browser"))

    await waitFor(() => expect(queryItem("Test")).not.toBeInTheDocument())
    expect(queryItem("Recording 01.wav")).not.toBeInTheDocument()
    expect(removeRoot.mock.calls).toEqual([["/samples/Test"]])
    expect(item("Factory")).toBeInTheDocument()
    // With no user folder left, the prompt is back.
    expect(screen.getByText(/Your own samples go here too/)).toBeInTheDocument()
  })

  it("shows why a folder could not be removed and keeps it", async () => {
    const { user } = await start(() => ({
      browserRemoveRoot: () => Promise.reject(new Error("Settings are locked")),
    }))
    await user.click(screen.getByRole("button", { name: "Add folder…" }))
    await findItem("Recording 01.wav")

    await openMenu("Test")
    fireEvent.click(menuItem("Remove from browser"))
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Could not remove Test", {
        description: "Settings are locked",
      })
    )
    expect(item("Test")).toBeInTheDocument()
  })
})

describe("folders in the right-click menu", () => {
  it("expands and collapses", async () => {
    await start()
    expect(await openMenu("Loops")).toEqual(["Expand", "Refresh", "Copy path"])
    fireEvent.click(menuItem("Expand"))
    expect(await findItem("Drum loop 128.wav")).toBeInTheDocument()

    await waitFor(() =>
      expect(screen.queryByRole("menu")).not.toBeInTheDocument()
    )
    await openMenu("Loops")
    fireEvent.click(menuItem("Collapse"))
    await waitFor(() =>
      expect(queryItem("Drum loop 128.wav")).not.toBeInTheDocument()
    )
  })

  it("copies a path", async () => {
    await start()
    const writeText = vi.fn(() => Promise.resolve())
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    })

    await openMenu("Loops")
    fireEvent.click(menuItem("Copy path"))
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith("/factory/Loops")
    )
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith("Path copied", {
        description: "/factory/Loops",
      })
    )
  })

  it("offers what can be done to the browser below the last row", async () => {
    await start()
    fireEvent(
      document.querySelector("[data-slot=browser-scroll]")!,
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        button: 2,
      })
    )
    const menu = await screen.findByRole("menu")
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((entry) => entry.textContent)
    ).toEqual([
      "Add folder to the browser…",
      "Refresh browser folder",
      "Collapse folders",
      "Expand loaded folders",
      "Filter the browserCtrl+F",
    ])
    // Previewing on selection is switched on and off from here too.
    expect(
      within(menu)
        .getAllByRole("menuitemcheckbox")
        .map((entry) => entry.textContent)
    ).toEqual(["Preview sounds when selected", "BrowserCtrl+B"])
  })
})

describe("dragging a sound out", () => {
  function dragStart(name: string) {
    const data = new Map<string, string>()
    const dataTransfer = {
      effectAllowed: "uninitialized",
      setData: vi.fn((type: string, value: string) => data.set(type, value)),
      setDragImage: vi.fn(),
    }
    const allowed = fireEvent.dragStart(item(name), { dataTransfer })
    return { allowed, data, dataTransfer }
  }

  it("carries the shared sample payload and a compact drag image", async () => {
    await openKicks()
    expect(item("Kick 02.wav")).toHaveAttribute("draggable", "true")

    const { allowed, data, dataTransfer } = dragStart("Kick 02.wav")
    expect(allowed).toBe(true)
    expect(JSON.parse(data.get(SAMPLE_DRAG_TYPE) ?? "null")).toEqual({
      path: KICK,
      name: "Kick 02",
      browser: expect.objectContaining({
        rootPath: "/factory",
        path: KICK,
        generation: expect.any(Number),
      }),
    })
    expect(dataTransfer.effectAllowed).toBe("copy")

    const [label, x, y] = dataTransfer.setDragImage.mock.calls[0] as [
      HTMLElement,
      number,
      number,
    ]
    expect(label.textContent).toBe("Kick 02")
    expect([x, y]).toEqual([10, 12])
    // The label is only needed for the browser's snapshot.
    await waitFor(() => expect(label.isConnected).toBe(false))
  })

  it("does not let folders be dragged", async () => {
    await openKicks()
    expect(item("Kicks")).toHaveAttribute("draggable", "false")
    const { allowed, data } = dragStart("Kicks")
    expect(allowed).toBe(false)
    expect(data.size).toBe(0)
  })
})

describe("actions", () => {
  it("are all in the registry, so the palette lists them", async () => {
    await start()
    const ids = registry.list().map((action) => action.id)
    for (const action of BROWSER_ACTIONS) expect(ids).toContain(action.id)
  })

  it("enable the selection actions only when a sound is selected", async () => {
    const { user } = await openKicks()
    const enabled = (id: string) => {
      const action = registry.get(id)
      // These actions read the stores themselves and ignore the argument.
      return action?.enabled?.(undefined as never) ?? true
    }
    expect(enabled("browser.addSelectedToRack")).toBe(false)
    expect(enabled("browser.previewSelected")).toBe(false)

    await user.click(item("Kick 02.wav"))
    expect(enabled("browser.addSelectedToRack")).toBe(true)
    expect(enabled("browser.previewSelected")).toBe(true)
    expect(enabled("browser.replaceChannelSample")).toBe(false)

    useUiStore.getState().selectChannel(channels()[0].id)
    expect(enabled("browser.replaceChannelSample")).toBe(true)
  })

  it("add the selected sound to the rack from anywhere", async () => {
    const { user, backend } = await openKicks()
    const add = vi.spyOn(backend, "addChannelFromFile")
    await user.click(item("Kick 02.wav"))

    await registry.get("browser.addSelectedToRack")?.run()
    expect(add.mock.calls.map(([path]) => [path])).toEqual([[KICK]])
  })

  it("show the panel and focus the filter", async () => {
    await start()
    useUiStore.getState().setPanelVisible("browser", false)
    await registry.get("browser.focusSearch")?.run()

    expect(useUiStore.getState().panels.browser).toBe(true)
    await waitFor(() =>
      expect(
        screen.getByRole("searchbox", { name: "Filter the browser" })
      ).toHaveFocus()
    )
  })
})
