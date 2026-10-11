import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type {
  MixerTrackPreset,
  PluginBinding,
  PluginEntry,
  PluginManagerState,
} from "@/bindings"
import { setBackend, type Backend } from "@/lib/ipc"
import { mockMixerPresetLibrary, presetFileStem } from "@/lib/ipc/mixer-presets"
import { SimDocument } from "@/lib/ipc/sim/document"
import { dispatch, redo, undo } from "@/lib/store/project"
import { startTestApp } from "@/test/harness"

import { flush, history, project, trackNamed } from "./test-utils"
import { missingPlugins, TrackPresetsPanel } from "./track-presets"

let app: Awaited<ReturnType<typeof startTestApp>>
/** The library's files, by name, as the mock shell holds them. */
let files: Map<string, string>
let installed: PluginEntry[]
let backend: Backend

beforeEach(async () => {
  app = await startTestApp()
  files = new Map()
  installed = []
  const plugins = app.backend.pluginsState.bind(app.backend)
  backend = {
    ...app.backend,
    ...mockMixerPresetLibrary(files),
    pluginsState: async (): Promise<PluginManagerState> => ({
      ...(await plugins()),
      entries: installed,
    }),
  }
  setBackend(backend)
})
afterEach(() => app.stop())

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((onResolve) => {
    resolve = onResolve
  })
  return { promise, resolve }
}

async function openPanel(
  user: ReturnType<typeof userEvent.setup>,
  track: string
) {
  const view = render(<TrackPresetsPanel track={trackNamed(track).id} />)
  await user.click(screen.getByRole("button", { name: "Track presets" }))
  await screen.findByRole("region", { name: "Mixer preset library" })
  await flush()
  return view
}

const status = () => screen.getByRole("status")
const kinds = (name: string) =>
  trackNamed(name).effects.map((slot) => slot.params.type)

async function fileRoundTrip() {
  const snapshot = await app.backend.documentSnapshot()
  const doc = SimDocument.create(snapshot.project)
  const reopened = SimDocument.open(doc.fileText())
  expect(reopened.project()).toEqual(snapshot.project)
  doc.dispose()
  reopened.dispose()
}

function hosted(name: string, id: string): PluginBinding {
  return {
    target: { type: "effect", effect: 0 },
    format: "clap",
    path: `/plugins/${id}.clap`,
    id,
    name,
    state: [9, 8, 7, 6],
    parameters: [],
    auxiliaryInputs: [],
  }
}

describe("mixer track presets: library, files, shelf, missing plugins and history", () => {
  it("names library files the way the desktop shell does", () => {
    expect(presetFileStem("Vocal chain")).toBe("Vocal chain")
    expect(presetFileStem('Drums: "tight" <1/2>')).toBe("Drums_ _tight_ _1_2_")
    expect(presetFileStem("  ..hidden.. ")).toBe("hidden")
    expect(presetFileStem("")).toBe("Mixer track")
    expect(presetFileStem("con")).toBe("_con")
    expect(presetFileStem("LPT1.backup")).toBe("_LPT1.backup")
    expect(presetFileStem("Console")).toBe("Console")
  })

  it("saves a track into the library, lists it, applies it to another track with fresh effects, and undoes in one step", async () => {
    const user = userEvent.setup()
    const kick = trackNamed("Kick").id
    await dispatch({ type: "addEffect", track: kick, kind: "eq" })
    await dispatch({ type: "addEffect", track: kick, kind: "compressor" })
    await dispatch({
      type: "updateMixerTrack",
      id: kick,
      patch: { volume: 0.5, pan: -0.25, latencyOffsetMs: 3 },
    })
    const source = structuredClone(trackNamed("Kick"))

    const first = await openPanel(user, "Kick")
    expect(screen.getByText(/No presets in .* yet/)).toBeVisible()
    const name = screen.getByRole("textbox", { name: "Mixer preset name" })
    await user.clear(name)
    await user.type(name, "Drum bus")
    await user.click(screen.getByRole("button", { name: "Save to library" }))
    await waitFor(() =>
      expect(status()).toHaveTextContent(
        "Saved Drum bus to the preset library as Drum bus.wfmixer"
      )
    )
    const list = await screen.findByRole("list", { name: "Library presets" })
    expect(
      within(list).getByRole("button", { name: "Drum bus · 2 FX" })
    ).toBeEnabled()
    expect([...files.keys()]).toEqual(["Drum bus.wfmixer"])
    // Saving changed nothing in the project.
    expect(trackNamed("Kick")).toEqual(source)

    // The same name again takes the next number instead of overwriting.
    await user.click(screen.getByRole("button", { name: "Save to library" }))
    await waitFor(() => expect(files.size).toBe(2))
    expect([...files.keys()]).toContain("Drum bus 2.wfmixer")
    first.unmount()

    // Another track, another panel: the library is still there to browse.
    await openPanel(user, "Clap")
    const before = structuredClone(project())
    const cursor = history().cursor
    const rows = await screen.findAllByRole("button", {
      name: "Drum bus · 2 FX",
    })
    expect(rows).toHaveLength(2)
    await user.click(rows[0])
    await waitFor(() => expect(status()).toHaveTextContent("Loaded Drum bus"))
    await flush()
    const clap = trackNamed("Clap")
    expect(kinds("Clap")).toEqual(["eq", "compressor"])
    expect(clap.volume).toBe(0.5)
    expect(clap.pan).toBe(-0.25)
    expect(clap.latencyOffsetMs).toBe(3)
    // Its own name, routing and dock stay; the effects are new ones.
    const was = before.mixer.tracks.find((track) => track.id === clap.id)!
    expect(clap.name).toBe("Clap")
    expect(clap.output).toBe(was.output)
    expect(clap.sends).toEqual(was.sends)
    const sourceIds = source.effects.map((slot) => slot.id)
    for (const slot of clap.effects) expect(sourceIds).not.toContain(slot.id)
    expect(clap.effects.map((slot) => slot.params)).toEqual(
      source.effects.map((slot) => slot.params)
    )
    expect(trackNamed("Kick")).toEqual(source)
    expect(history().cursor).toBe(cursor + 1)
    await fileRoundTrip()

    await act(async () => {
      await undo()
    })
    expect(project()).toEqual({ ...before, nextId: project().nextId })
    await act(async () => {
      await redo()
    })
    expect(kinds("Clap")).toEqual(["eq", "compressor"])
  })

  it("deletes a library file only after a second, explicit click", async () => {
    const user = userEvent.setup()
    await openPanel(user, "Kick")
    await user.click(screen.getByRole("button", { name: "Save to library" }))
    await screen.findByRole("button", { name: "Kick · 0 FX" })
    await user.click(
      screen.getByRole("button", { name: "Delete library preset Kick" })
    )
    expect(files.size).toBe(1)
    await user.click(
      screen.getByRole("button", {
        name: "Confirm deleting library preset Kick",
      })
    )
    await waitFor(() =>
      expect(status()).toHaveTextContent(
        "Deleted Kick.wfmixer from the preset library"
      )
    )
    expect(files.size).toBe(0)
    expect(await screen.findByText(/No presets in .* yet/)).toBeVisible()
  })

  it("lists a damaged or newer file with its reason, cannot apply it, and still applies its neighbours", async () => {
    const user = userEvent.setup()
    const good = await app.backend.mixerPresetCapture(
      trackNamed("Kick").id,
      (await app.backend.timelineState()).generation,
      (await app.backend.timelineState()).revision
    )
    files.set("Good.wfmixer", JSON.stringify({ ...good, name: "Good" }))
    files.set("Broken.wfmixer", '{"version": 1, "name": "Cut sho')
    files.set(
      "Newer.wfmixer",
      JSON.stringify({ ...good, name: "From the future", version: 2 })
    )
    files.set("notes.txt", "not a preset")
    await openPanel(user, "Clap")
    const list = await screen.findByRole("list", { name: "Library presets" })
    expect(within(list).getAllByRole("listitem")).toHaveLength(3)
    expect(
      within(list).getByRole("button", { name: "Broken · 0 FX" })
    ).toBeDisabled()
    expect(
      within(list).getByText(/Broken\.wfmixer: Invalid mixer preset/)
    ).toBeVisible()
    expect(
      within(list).getByRole("button", { name: "Newer · 0 FX" })
    ).toBeDisabled()
    expect(
      within(list).getByText(
        /Newer\.wfmixer: Unsupported mixer preset version/
      )
    ).toBeVisible()
    await expect(
      backend.mixerPresetLibraryRead("Broken.wfmixer")
    ).rejects.toThrow("Invalid mixer preset")
    await expect(
      backend.mixerPresetLibraryRead("../outside.wfmixer")
    ).rejects.toThrow("not a preset of the mixer preset library")

    const cursor = history().cursor
    await user.click(within(list).getByRole("button", { name: "Good · 0 FX" }))
    await waitFor(() => expect(status()).toHaveTextContent("Loaded Good"))
    expect(history().cursor).toBe(cursor + 1)
  })

  it("says which hosted plugins are missing before and after loading, and keeps their state with the project", async () => {
    const user = userEvent.setup()
    const kick = trackNamed("Kick").id
    await dispatch({
      type: "addPluginEffect",
      track: kick,
      plugin: hosted("Tape Echo", "vendor.tape-echo"),
    })
    await dispatch({
      type: "addPluginEffect",
      track: kick,
      plugin: hosted("Wide Verb", "vendor.wide-verb"),
    })
    // Only one of the two is installed and usable on this machine.
    installed = [
      {
        path: "/elsewhere/Tape Echo.clap",
        format: "CLAP",
        id: "vendor.tape-echo",
        name: "Tape Echo",
        vendor: "Vendor",
        instrument: false,
        usable: true,
        error: null,
      },
      {
        path: "/plugins/vendor.wide-verb.clap",
        format: "clap",
        id: "vendor.wide-verb",
        name: "Wide Verb",
        vendor: "Vendor",
        instrument: false,
        usable: false,
        error: "The plugin crashed while it was scanned",
      },
    ]
    expect(
      missingPlugins(
        [
          { format: "clap", id: "vendor.tape-echo", name: "Tape Echo" },
          { format: "clap", id: "vendor.wide-verb", name: "Wide Verb" },
          { format: "vst3", id: "vendor.tape-echo", name: "Tape Echo VST3" },
          { format: "clap", id: "vendor.wide-verb", name: "Wide Verb" },
        ],
        installed
      )
    ).toEqual(["Wide Verb", "Tape Echo VST3"])

    const first = await openPanel(user, "Kick")
    await user.click(screen.getByRole("button", { name: "Save to library" }))
    const list = await screen.findByRole("list", { name: "Library presets" })
    // Before loading: the row names what is not installed here.
    expect(
      await within(list).findByText("Not installed here: Wide Verb")
    ).toBeVisible()
    first.unmount()

    await openPanel(user, "Clap")
    const before = structuredClone(project())
    await user.click(await screen.findByRole("button", { name: "Kick · 2 FX" }))
    // After loading: the same, with what happens to them.
    await waitFor(() =>
      expect(status()).toHaveTextContent(
        "Loaded Kick. Not installed here: Wide Verb. Their settings are kept with the project, and those effects pass audio through until the plugins are installed."
      )
    )
    await flush()
    // Both bindings came across, on the new effects, with their state.
    const clap = trackNamed("Clap")
    expect(clap.effects).toHaveLength(2)
    const bound = (project().plugins ?? []).filter(
      (plugin) =>
        plugin.target.type === "effect" &&
        clap.effects.some(
          (slot) =>
            plugin.target.type === "effect" && slot.id === plugin.target.effect
        )
    )
    expect(bound.map((plugin) => plugin.id).sort()).toEqual([
      "vendor.tape-echo",
      "vendor.wide-verb",
    ])
    for (const plugin of bound) expect(plugin.state).toEqual([9, 8, 7, 6])
    // The source keeps its own bindings.
    expect((project().plugins ?? []).length).toBe(
      (before.plugins ?? []).length + 2
    )
    await fileRoundTrip()
    await act(async () => {
      await undo()
    })
    expect(project().plugins ?? []).toEqual(before.plugins ?? [])
    expect(trackNamed("Clap").effects).toEqual([])
  })

  it("refuses a preset when the destination changed while its file was being read", async () => {
    const user = userEvent.setup()
    await openPanel(user, "Kick")
    await user.click(screen.getByRole("button", { name: "Save to library" }))
    const row = await screen.findByRole("button", { name: "Kick · 0 FX" })

    const reading = deferred<void>()
    const read = backend.mixerPresetLibraryRead.bind(backend)
    const slow = vi.fn(async (file: string) => {
      const preset = await read(file)
      await reading.promise
      return preset
    })
    setBackend({ ...backend, mixerPresetLibraryRead: slow })
    await user.click(row)
    await waitFor(() => expect(slow).toHaveBeenCalled())
    // The destination is edited while the file is still on its way.
    await act(async () => {
      await dispatch({
        type: "updateMixerTrack",
        id: trackNamed("Kick").id,
        patch: { volume: 0.25 },
      })
    })
    const edited = structuredClone(project())
    const cursor = history().cursor
    reading.resolve()
    await waitFor(() =>
      expect(status()).toHaveTextContent(
        "Preset was not applied. The destination may have changed."
      )
    )
    expect(project()).toEqual(edited)
    expect(history().cursor).toBe(cursor)
  })

  it("does not save a capture that finished after another project replaced this one", async () => {
    const user = userEvent.setup()
    await openPanel(user, "Kick")
    const capturing = deferred<void>()
    const capture = backend.mixerPresetCapture.bind(backend)
    const slow = vi.fn(async (...args: Parameters<Backend["mixerPresetCapture"]>) => {
      const preset = await capture(...args)
      await capturing.promise
      return preset
    })
    setBackend({ ...backend, mixerPresetCapture: slow })
    await user.click(screen.getByRole("button", { name: "Save to library" }))
    await waitFor(() => expect(slow).toHaveBeenCalled())
    await act(async () => {
      await backend.projectNew()
    })
    capturing.resolve()
    await waitFor(() =>
      expect(status()).toHaveTextContent(
        "The project changed during preset capture"
      )
    )
    expect(files.size).toBe(0)
  })

  it("keeps the shelf and single files working, reports a full shelf store and a cancelled dialog without claiming success", async () => {
    const user = userEvent.setup()
    await dispatch({
      type: "addEffect",
      track: trackNamed("Kick").id,
      kind: "limiter",
    })
    let saved: MixerTrackPreset | null = null
    const save = vi.fn(async (preset: MixerTrackPreset) => {
      saved = preset
      return "/exports/Kick.wfmixer" as string | null
    })
    const load = vi.fn(async () => saved)
    setBackend({ ...backend, mixerPresetSave: save, mixerPresetLoad: load })
    await openPanel(user, "Kick")

    // Shelf.
    await user.click(screen.getByRole("button", { name: "Save to shelf" }))
    await waitFor(() =>
      expect(status()).toHaveTextContent("Saved Kick to the preset shelf")
    )
    const shelf = screen.getByRole("list", { name: "Saved mixer presets" })
    expect(within(shelf).getByRole("button", { name: "Kick · 1 FX" }))
      .toBeVisible()

    // A store that is full: the error is shown and nothing is added.
    const quota = vi
      .spyOn(Storage.prototype, "setItem")
      .mockImplementation(() => {
        throw new DOMException("The quota has been exceeded.", "QuotaExceededError")
      })
    await user.click(screen.getByRole("button", { name: "Save to shelf" }))
    await waitFor(() => expect(status()).toHaveTextContent(/quota/i))
    expect(within(shelf).getAllByRole("listitem")).toHaveLength(1)
    quota.mockRestore()

    // Single file: saved, then a cancelled save says nothing.
    await user.click(screen.getByRole("button", { name: "Save file…" }))
    await waitFor(() =>
      expect(status()).toHaveTextContent("Saved /exports/Kick.wfmixer")
    )
    save.mockResolvedValueOnce(null)
    await user.click(screen.getByRole("button", { name: "Save file…" }))
    await waitFor(() => expect(save).toHaveBeenCalledTimes(2))
    await flush()
    expect(screen.queryByRole("status")).toBeNull()

    // A cancelled load changes nothing; a chosen file loads.
    const before = structuredClone(project())
    const cursor = history().cursor
    load.mockResolvedValueOnce(null)
    await user.click(screen.getByRole("button", { name: "Load file…" }))
    await waitFor(() => expect(load).toHaveBeenCalledTimes(1))
    await flush()
    expect(project()).toEqual(before)
    expect(history().cursor).toBe(cursor)
    await user.click(screen.getByRole("button", { name: "Load file…" }))
    await waitFor(() => expect(status()).toHaveTextContent("Loaded Kick"))
    expect(history().cursor).toBe(cursor + 1)
    expect(kinds("Kick")).toEqual(["limiter"])

    // Name and color come along only when asked for.
    await user.click(
      screen.getByRole("checkbox", { name: "Restore preset name and color" })
    )
    saved = { ...saved!, name: "Renamed", color: 0x123456 }
    await user.click(screen.getByRole("button", { name: "Load file…" }))
    await waitFor(() => expect(status()).toHaveTextContent("Loaded Renamed"))
    await flush()
    expect(trackNamed("Renamed").color).toBe(0x123456)
  })
})
