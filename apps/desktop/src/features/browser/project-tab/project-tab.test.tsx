import { act, fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"

import type { Project } from "@/bindings/Project"
import {
  effectDescriptor,
  instrumentDescriptor,
} from "@/features/params/descriptors"
import { historyLabel } from "@/lib/i18n"
import { useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"

import BrowserPanel from ".."
import { useLibraryStore } from "../library-store"
import { setFilter, useBrowserStore } from "../store"
import { findItem, item, startBrowserTest } from "../testing"
import { ProjectTab } from "."

let stop = () => {}
afterEach(() => stop())

function populatedProject(): Project {
  const project = useProjectStore.getState().project
  return {
    ...project,
    patterns: [
      ...project.patterns,
      { id: 101, name: "Bridge idea", color: 0, lengthSteps: 16, lanes: [] },
    ],
    channels: [
      {
        id: 102,
        name: "Lead voice",
        color: 0,
        volume: 1,
        pan: 0,
        muted: false,
        solo: false,
        mixerTrack: 0,
        source: {
          type: "instrument",
          params: instrumentDescriptor("subtractiveSynth").defaults,
        },
      },
    ],
    samples: [
      {
        id: 103,
        name: "Room hit",
        path: { kind: "project", path: "room.wav" },
      },
    ],
    mixer: {
      ...project.mixer,
      tracks: project.mixer.tracks.map((track, index) =>
        index === 0
          ? {
              ...track,
              name: "Main bus",
              effects: [
                {
                  id: 104,
                  enabled: true,
                  mix: 1,
                  params: effectDescriptor("reverb").defaults,
                },
              ],
            }
          : track
      ),
    },
    automations: [
      {
        id: 105,
        name: "Volume rise",
        color: 0,
        target: { type: "channelVolume", channel: 102 },
        points: [{ tick: 0, value: 0.5, curve: 0, hold: false }],
      },
    ],
  }
}

describe("read-only project browser", () => {
  it("shows project names and history oldest first, with entries at the cursor undone", async () => {
    const app = await startBrowserTest()
    stop = app.stop
    const dispatch = vi.spyOn(app.backend, "dispatch")
    const undo = vi.spyOn(app.backend, "undo")
    const historyJump = vi.spyOn(app.backend, "historyJump")
    const project = populatedProject()
    const history = {
      entries: [{ label: "Add lead" }, { label: "Add room hit" }],
      cursor: 1,
    }
    act(() => useProjectStore.setState({ project, history }))
    const selection = useUiStore.getState()
    const document = useProjectStore.getState()
    render(<BrowserPanel />)
    const user = userEvent.setup()
    await user.click(screen.getByRole("tab", { name: "Project" }))
    const panel = screen.getByRole("tabpanel", { name: "Project" })

    for (const [group, name] of [
      ["Patterns", "Bridge idea"],
      ["Channels", "Lead voice"],
      ["Samples", "Room hit"],
      ["Automations", "Volume rise"],
    ]) {
      expect(
        within(within(panel).getByRole("region", { name: group })).getByText(
          name
        )
      ).toBeInTheDocument()
    }
    const effects = within(panel).getByRole("region", { name: "Mixer effects" })
    expect(
      within(effects).getByRole("heading", { name: "Main bus" })
    ).toBeInTheDocument()
    expect(
      within(effects).getByText(effectDescriptor("reverb").name)
    ).toBeInTheDocument()
    const rows = within(
      within(panel).getByRole("region", { name: "Edit history" })
    ).getAllByRole("listitem")
    expect(rows[0]).toHaveTextContent("Add lead")
    expect(rows[0]).toHaveAttribute("data-undone", "false")
    expect(within(rows[0]).getByText("Applied")).toBeInTheDocument()
    expect(rows[1]).toHaveTextContent("Add room hit")
    expect(rows[1]).toHaveAttribute("data-undone", "true")
    expect(within(rows[1]).getByText("Undone")).toHaveAttribute(
      "data-variant",
      "outline"
    )

    await user.click(within(panel).getByText("Bridge idea"))
    await user.dblClick(rows[1])
    fireEvent.contextMenu(rows[1])
    fireEvent.keyDown(panel, { key: "Enter" })
    fireEvent.keyDown(panel, { key: "Delete" })
    expect(within(panel).getAllByRole("button").map(button => button.textContent)).toEqual(["Pattern 1", "Bridge idea", "Lead voice", "Volume rise"])
    expect(screen.queryByRole("menu")).toBeNull()
    expect(dispatch).not.toHaveBeenCalled()
    expect(undo).not.toHaveBeenCalled()
    expect(historyJump).not.toHaveBeenCalled()
    expect(useProjectStore.getState()).toBe(document)
    expect(useUiStore.getState()).toEqual({ ...selection, centerTab: "channelRack" })
  })

  it("refreshes document lists and cursor status from store updates", async () => {
    const app = await startBrowserTest()
    stop = app.stop
    act(() =>
      useProjectStore.setState({
        project: populatedProject(),
        history: { entries: [{ label: "Add lead" }], cursor: 1 },
      })
    )
    render(<ProjectTab />)
    act(() => {
      const { project } = useProjectStore.getState()
      useProjectStore.setState({
        project: {
          ...project,
          patterns: project.patterns.map((pattern) =>
            pattern.id === 101
              ? { ...pattern, name: "Changed bridge" }
              : pattern
          ),
          samples: [],
        },
        history: { entries: [{ label: "Add lead" }], cursor: 0 },
      })
    })
    expect(screen.getByText("Changed bridge")).toBeInTheDocument()
    expect(screen.queryByText("Bridge idea")).toBeNull()
    expect(screen.queryByRole("region", { name: "Samples" })).toBeNull()
    expect(screen.getByText("Undone").closest("li")).toHaveAttribute(
      "data-undone",
      "true"
    )
  })

  it("shows a kept history label as words, not its stored form", async () => {
    const app = await startBrowserTest()
    stop = app.stop
    act(() =>
      useProjectStore.setState({
        project: populatedProject(),
        history: { entries: [{ label: historyLabel("Fill every {every} steps", { every: 4 }) }], cursor: 1 },
      })
    )
    render(<ProjectTab />)
    expect(screen.getByText("Fill every 4 steps")).toBeInTheDocument()
    expect(screen.queryByText(/windfall-label/)).toBeNull()
  })

  it("omits empty groups and shows optional plugin names and retained names when present", async () => {
    const app = await startBrowserTest()
    stop = app.stop
    const project = useProjectStore.getState().project
    act(() =>
      useProjectStore.setState({
        project: {
          ...project,
          patterns: [],
          channels: [],
          samples: [],
          automations: [],
          plugins: [],
          retainedPlugins: [],
          mixer: { tracks: [] },
        },
        history: { entries: [], cursor: 0 },
      })
    )
    render(<ProjectTab />)
    expect(
      screen
        .getAllByRole("region")
        .map((region) => region.getAttribute("aria-labelledby"))
    ).toHaveLength(1)
    expect(screen.getByRole("region", { name: "Patterns" })).toBeInTheDocument()
    for (const plugins of [undefined, []]) {
      act(() =>
        useProjectStore.setState({
          project: {
            ...useProjectStore.getState().project,
            plugins,
            retainedPlugins: plugins,
          },
        })
      )
      expect(screen.queryByRole("region", { name: "Plugins" })).toBeNull()
      expect(
        screen.queryByRole("region", { name: "Retained plugin states" })
      ).toBeNull()
    }
    act(() =>
      useProjectStore.setState({
        project: {
          ...useProjectStore.getState().project,
          plugins: [
            {
              target: { type: "instrument", channel: 102 },
              format: "clap",
              path: "missing.clap",
              id: "native-lead",
              name: "Native lead",
              state: [1],
              parameters: [],
            },
          ],
          retainedPlugins: [
            { source: "import", internalName: "Opaque synth", state: [2] },
            {
              source: "import",
              internalName: "opaque-reverb",
              name: "Saved room",
              state: [3],
            },
          ],
        },
      })
    )
    expect(
      within(screen.getByRole("region", { name: "Plugins" })).getByText(
        "Native lead"
      )
    ).toBeInTheDocument()
    const retained = within(
      screen.getByRole("region", { name: "Retained plugin states" })
    )
    expect(retained.getByText("Opaque synth")).toBeInTheDocument()
    expect(retained.getByText("Saved room")).toBeInTheDocument()
  })

  it("restores the library tree, filter and tag choices after leaving Project", async () => {
    const app = await startBrowserTest()
    stop = app.stop
    await app.backend.librarySetMetadata("/factory/Drums/Kicks/Kick 02.wav", {
      favorite: true,
      tags: ["warm"],
    })
    render(<BrowserPanel />)
    const user = userEvent.setup()
    await user.click(await findItem("Drums"))
    await user.click(await findItem("Kicks"))
    await findItem("Kick 02.wav")
    const expanded = useBrowserStore.getState().expanded
    act(() => {
      setFilter("kick")
      useLibraryStore.setState({ tags: ["warm"], favoritesOnly: true })
    })
    await user.click(await findItem("Kick 02.wav"))
    const selected = useBrowserStore.getState().selected
    await user.click(screen.getByRole("tab", { name: "Project" }))
    expect(screen.queryByRole("searchbox")).toBeNull()
    await user.click(screen.getByRole("tab", { name: "Library" }))
    expect(
      screen.getByRole("searchbox", { name: "Filter the browser" })
    ).toHaveValue("kick")
    expect(
      screen.getByRole("combobox", { name: "Filter by tag" })
    ).toHaveTextContent("warm")
    expect(
      screen.getByRole("button", { name: "Favorites only" })
    ).toHaveAttribute("aria-pressed", "true")
    expect(await findItem("Kick 02.wav")).toHaveAttribute(
      "aria-selected",
      "true"
    )
    expect(useBrowserStore.getState().selected).toBe(selected)
    expect(useBrowserStore.getState().expanded).toBe(expanded)
    act(() => {
      setFilter("")
      useLibraryStore.setState({ tags: [], favoritesOnly: false })
    })
    expect(await findItem("Kick 01.wav")).toBeInTheDocument()
    expect(item("Drums")).toHaveAttribute("aria-expanded", "true")
    expect(item("Kicks")).toHaveAttribute("aria-expanded", "true")
  })
})
