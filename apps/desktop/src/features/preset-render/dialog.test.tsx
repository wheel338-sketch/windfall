import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { DispatchResult } from "@/bindings"
import { deferred, startBrowserTest } from "@/features/browser/testing"
import { addToRack } from "@/features/browser/commands"
import { addChannelFromFile, replaceSampleFromFile } from "@/features/channel-rack/channel-ops"
import { newProject } from "@/lib/flows/project"
import { useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { useLanguageStore } from "@/lib/i18n"
import { PresetRenderDialog, PresetRerenderButton, closePresetRender, openPresetRender } from "./dialog"
import { DEFAULT_RENDER, UNSUPPORTED_PRESET } from "./types"

let stop = () => {}
afterEach(() => { closePresetRender(); stop(); vi.restoreAllMocks(); act(() => useLanguageStore.setState({ language: "en" })) })
const wave = "/factory/Drums/Kicks/Kick 02.wav"
const preset = "/factory/Own voice.wfpreset"

describe("preset render dialog", () => {
  it("retranslates the mounted dialog and formats its rate without changing edited render controls", async () => {
    const app = await startBrowserTest(() => ({
      presetRenderBegin: async () => ({ request: 76, sampleRate: 48000 }),
      presetRenderCancel: async () => {},
    }))
    stop = app.stop
    render(<PresetRenderDialog />)
    act(() => { void openPresetRender({ path: preset }) })
    await screen.findByText("48,000 Hz (project rate)")
    fireEvent.change(screen.getByLabelText("MIDI note"), { target: { value: "72" } })
    act(() => useLanguageStore.setState({ language: "es" }))
    expect(screen.getByRole("dialog", { name: "Renderizar preset como muestra" })).toBeInTheDocument()
    expect(screen.getByText("48.000 Hz (frecuencia del proyecto)")).toBeInTheDocument()
    expect(screen.getByLabelText("Nota MIDI")).toHaveValue(72)
    expect(screen.getByRole("button", { name: "Renderizar como muestra" })).toBeEnabled()
  })
  it("mounts editable note, velocity, length, tail, project rate and mono/stereo controls and sends real import settings", async () => {
    const app = await startBrowserTest((mock) => ({
      presetRenderBegin: async () => ({ request: 71, sampleRate: 44100 }),
      presetRenderFile: async () => mock.addChannelFromFile(wave),
      presetRenderCancel: async () => {},
    }))
    stop = app.stop
    const user = userEvent.setup()
    const work = vi.spyOn(app.backend, "presetRenderFile")
    render(<PresetRenderDialog />)
    let pending!: Promise<void>
    act(() => { pending = addChannelFromFile(preset, 0) })
    await screen.findByRole("dialog", { name: "Render preset as sample" })
    await screen.findByText("44,100 Hz (project rate)")
    fireEvent.change(screen.getByLabelText("MIDI note"), { target: { value: "72" } })
    fireEvent.change(screen.getByLabelText("Velocity"), { target: { value: "0.6" } })
    fireEvent.change(screen.getByLabelText("Note length (seconds)"), { target: { value: "0.5" } })
    fireEvent.change(screen.getByLabelText("Release tail limit (seconds)"), { target: { value: "2" } })
    await user.click(screen.getByRole("button", { name: "Mono" }))
    await user.click(screen.getByRole("button", { name: "Render as sample" }))
    await pending
    expect(work).toHaveBeenCalledWith(71, { path: preset, index: 0,
      options: { note: 72, velocity: 0.6, lengthSecs: 0.5, tailSecs: 2, stereo: false } })
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument())
    expect(useProjectStore.getState().history.cursor).toBe(1)
    expect(useUiStore.getState().selectedChannel).not.toBeNull()
  })

  it("browser import carries its pinned token and sampler drop keeps its captured destination when selection changes", async () => {
    const app = await startBrowserTest((mock) => ({
      presetRenderBegin: async () => ({ request: 72, sampleRate: 48000 }),
      presetRenderFile: async (_request, input) => input.channel === undefined
        ? mock.addChannelFromFile(wave) : mock.setChannelSampleFromFile(input.channel, wave),
      presetRenderCancel: async () => {},
    }))
    stop = app.stop
    const token = await app.backend.libraryFile(wave)
    const work = vi.spyOn(app.backend, "presetRenderFile")
    render(<PresetRenderDialog />)
    const user = userEvent.setup()
    // The browser's own token validation remains in the ordinary route.
    const ownToken = { ...token, path: preset }
    let browser!: Promise<void>
    act(() => { browser = addToRack(preset, ownToken) })
    await screen.findByText("48,000 Hz (project rate)")
    await user.click(screen.getByRole("button", { name: "Render as sample" }))
    await browser
    expect(work).toHaveBeenLastCalledWith(72, { path: preset, browser: ownToken, options: DEFAULT_RENDER })
    const channel = useProjectStore.getState().project.channels[0].id
    let drop!: Promise<void>
    act(() => { drop = replaceSampleFromFile(channel, preset) })
    await screen.findByRole("dialog", { name: "Render preset as sample" })
    await waitFor(() => expect(screen.getByRole("button", { name: "Render as sample" })).toBeEnabled())
    useUiStore.getState().selectChannel(null)
    await user.click(screen.getByRole("button", { name: "Render as sample" }))
    await drop
    expect(work).toHaveBeenLastCalledWith(72, { path: preset, channel, options: DEFAULT_RENDER })
  })

  it("cancel dismisses a pending native job and rejects its late result without changing project or history", async () => {
    const native = deferred<DispatchResult>()
    const app = await startBrowserTest(() => ({
      presetRenderBegin: async () => ({ request: 73, sampleRate: 48000 }),
      presetRenderFile: () => native.promise,
      presetRenderCancel: async () => {},
    }))
    stop = app.stop
    const cancel = vi.spyOn(app.backend, "presetRenderCancel")
    const before = structuredClone(useProjectStore.getState().project)
    const history = structuredClone(useProjectStore.getState().history)
    const user = userEvent.setup()
    render(<PresetRenderDialog />)
    let pending!: Promise<void>
    act(() => { pending = addChannelFromFile(preset) })
    await screen.findByText("48,000 Hz (project rate)")
    await user.click(screen.getByRole("button", { name: "Render as sample" }))
    await screen.findByRole("status")
    await user.click(screen.getByRole("button", { name: "Cancel rendering" }))
    await pending
    expect(cancel).toHaveBeenCalledWith(73)
    const snapshot = await app.backend.documentSnapshot()
    await act(async () => { native.resolve({ created: [], patch: {
      revision: snapshot.revision + 1, channels: [], dirty: true, history: snapshot.history,
    } }); await Promise.resolve() })
    expect(useProjectStore.getState().project).toEqual(before)
    expect(useProjectStore.getState().history).toEqual(history)
  })

  it("project replacement cancels a dialog and native capability/empty-text failures remain visible", async () => {
    const app = await startBrowserTest(() => ({
      presetRenderBegin: async () => ({ request: 74, sampleRate: 48000 }),
      presetRenderFile: async () => { throw new Error("Speech preset text is empty") },
      presetRenderCancel: async () => {},
    }))
    stop = app.stop
    const cancel = vi.spyOn(app.backend, "presetRenderCancel")
    render(<PresetRenderDialog />)
    const user = userEvent.setup()
    act(() => { void openPresetRender({ path: preset }) })
    await screen.findByText("48,000 Hz (project rate)")
    await user.click(screen.getByRole("button", { name: "Render as sample" }))
    expect(await screen.findByText(/Speech preset text is empty/)).toBeInTheDocument()
    await act(() => newProject())
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument())
    expect(cancel).toHaveBeenCalledWith(74)
  })

  it.each(["syn", "speech"])("shows the explicit proprietary .%s message and Windfall equivalents without starting DSP", async (extension) => {
    const app = await startBrowserTest()
    stop = app.stop
    const begin = vi.spyOn(app.backend, "presetRenderBegin")
    const work = vi.spyOn(app.backend, "presetRenderFile")
    render(<PresetRenderDialog />)
    let pending!: Promise<void>
    act(() => { pending = addChannelFromFile(`/factory/Unsupported.${extension}`) })
    expect(await screen.findByRole("dialog", { name: "Unsupported preset source" })).toBeInTheDocument()
    expect(screen.getByText(UNSUPPORTED_PRESET)).toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Render as sample" })).not.toBeInTheDocument()
    expect(begin).not.toHaveBeenCalled()
    expect(work).not.toHaveBeenCalled()
    await userEvent.setup().click(screen.getByRole("button", { name: "Cancel" }))
    await pending
  })

  it("browser mock reports unavailable DSP instead of a successful render", async () => {
    const app = await startBrowserTest()
    stop = app.stop
    render(<PresetRenderDialog />)
    act(() => { void openPresetRender({ path: preset }) })
    expect(await screen.findByText(/Preset audio rendering is available in the desktop app/)).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Render as sample" })).toBeDisabled()
  })

  it("mounted rerender control uses the retained source identity and saved controls", async () => {
    const app = await startBrowserTest(() => ({
      presetRenderBegin: async () => ({ request: 75, sampleRate: 48000 }),
      presetRenderFile: async () => { throw new Error("Test worker admission") },
      presetRenderCancel: async () => {},
    }))
    stop = app.stop
    const project = structuredClone(useProjectStore.getState().project)
    const channel = project.channels.find((c) => c.source.type === "sampler" && c.source.sample !== null)!
    if (channel.source.type !== "sampler") throw new Error("sampler fixture")
    const source = { ...project.samples[0], id: project.nextId, path: { kind: "project", path: "Presets/Own.wfpreset" } as const }
    const settings = { ...DEFAULT_RENDER, note: 57, stereo: false }
    project.samples.push(source)
    project.settings = { ...project.settings, presetRenders: [{ sample: channel.source.sample!, source: source.id, settings }] } as typeof project.settings
    useProjectStore.setState({ project })
    const work = vi.spyOn(app.backend, "presetRenderFile")
    render(<><PresetRerenderButton channel={channel.id} /><PresetRenderDialog /></>)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Re-render preset…" }))
    await screen.findByText("48,000 Hz (project rate)")
    expect(screen.getByLabelText("MIDI note")).toHaveValue(57)
    await user.click(screen.getByRole("button", { name: "Render as sample" }))
    await screen.findByText(/Test worker admission/)
    expect(work).toHaveBeenCalledWith(75, { path: source.path.path, source: source.id, channel: channel.id, options: settings })
  })
})
