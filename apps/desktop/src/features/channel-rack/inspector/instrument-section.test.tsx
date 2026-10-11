import { act, fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { ChannelId, InstrumentKind, PluginBinding } from "@/bindings"
import { ValueContextMenus } from "@/components/value-context-menu"
import { instrumentDescriptor, paramIndex } from "@/features/params"
import { isInstrumentChannel } from "@/lib/channel-source"
import type { MockBackend } from "@/lib/ipc/mock"
import { dispatch, undo, useProjectStore } from "@/lib/store/project"
import { settle, startTestApp } from "@/test/harness"

import { InstrumentSection } from "./instrument-section"

// The section only needs this naming helper, not the rack's action registration.
vi.mock("../actions", () => ({
  synthSoundActionId: (id: string) => `channel.synthSound.${id}`,
}))

let backend: MockBackend
let stop: () => void

beforeEach(async () => {
  ;({ backend, stop } = await startTestApp())
})

afterEach(() => {
  stop()
  vi.restoreAllMocks()
})

const flush = () => act(() => settle())
const history = () => useProjectStore.getState().history

function channelOf(id: ChannelId) {
  const channel = useProjectStore
    .getState()
    .project.channels.find((item) => item.id === id)
  if (!channel || !isInstrumentChannel(channel))
    throw new Error("No instrument channel")
  return channel
}

function Panel({ id }: { id: ChannelId }) {
  const channel = useProjectStore((state) =>
    state.project.channels.find((item) => item.id === id)
  )
  return channel && isInstrumentChannel(channel) ? (
    <InstrumentSection channel={channel} />
  ) : null
}

async function open(kind: InstrumentKind) {
  const result = await dispatch({ type: "addChannel", instrument: kind })
  if (!result) throw new Error(`Could not add ${kind}`)
  const id = result.created[0]
  // Use the catalog's complete defaults, including the sequenced bass steps.
  await dispatch({
    type: "setInstrumentParams",
    channel: id,
    params: instrumentDescriptor(kind).defaults,
  })
  render(
    <ValueContextMenus>
      <Panel id={id} />
    </ValueContextMenus>
  )
  await flush()
  return id
}

describe("instrument settings panels", () => {
  it("keeps the subtractive synth's oscillator controls and sound menu", async () => {
    await open("subtractiveSynth")
    const oscillator = screen.getByRole("group", { name: "Oscillator 1" })
    expect(
      within(oscillator).getByRole("slider", { name: "Osc 1 level" })
    ).toBeVisible()
    expect(screen.getByRole("button", { name: /^Sound:/ })).toBeVisible()
    expect(
      document.querySelector("[data-slot=synth-editor]")
    ).toBeInTheDocument()
  })

  it("shows acidLine's own descriptor controls without the subtractive oscillator panel", async () => {
    const id = await open("acidLine")
    const descriptor = instrumentDescriptor("acidLine")
    expect(channelOf(id).source.params).toEqual(descriptor.defaults)
    const editor = screen.getByRole("group", {
      name: `${descriptor.name} settings`,
    })
    const info = descriptor.params[paramIndex(descriptor, "cutoffHz")]
    expect(
      within(editor).getByRole("slider", { name: info.name })
    ).toHaveAttribute("aria-valuenow", String(info.default))
    expect(
      new Set(
        [...editor.querySelectorAll<HTMLElement>("[data-param]")].map(
          (control) => control.dataset.param
        )
      )
    ).toEqual(new Set(descriptor.params.map((info) => info.id)))
    expect(screen.queryByRole("group", { name: "Oscillator 1" })).toBeNull()
    expect(screen.queryByText("Osc 1")).toBeNull()
    expect(screen.queryByRole("button", { name: /^Sound:/ })).toBeNull()
    expect(document.querySelector("[data-slot=synth-editor]")).toBeNull()
  })

  it("dispatches a descriptor control change with its instrument parameter index", async () => {
    const channel = await open("acidLine")
    const descriptor = instrumentDescriptor("acidLine")
    const param = paramIndex(descriptor, "accentAmount")
    const info = descriptor.params[param]
    const sent = vi.spyOn(backend, "dispatch")
    const control = screen.getByRole("slider", { name: info.name })
    fireEvent.keyDown(control, { key: "End" })
    fireEvent.keyUp(control, { key: "End" })
    await flush()
    expect(sent).toHaveBeenLastCalledWith(
      { type: "setInstrumentParam", channel, param, value: info.max },
      expect.any(Number)
    )
    expect(control).toHaveAttribute("aria-valuenow", String(info.max))
  })

  it("groups all changes of an instrument control drag into one undo step", async () => {
    const channel = await open("acidLine")
    const descriptor = instrumentDescriptor("acidLine")
    const param = paramIndex(descriptor, "accentAmount")
    const control = screen.getByRole("slider", {
      name: descriptor.params[param].name,
    })
    const original = channelOf(channel).source.params
    const cursor = history().cursor
    const sent = vi.spyOn(backend, "dispatch")
    fireEvent.pointerDown(control, { pointerId: 1, button: 0, clientY: 300 })
    for (const clientY of [290, 280, 260, 240]) {
      fireEvent.pointerMove(control, { pointerId: 1, clientY })
    }
    fireEvent.pointerUp(control, { pointerId: 1 })
    await flush()
    const moves = sent.mock.calls.filter(
      ([command]) => command.type === "setInstrumentParam"
    )
    expect(moves).toHaveLength(4)
    for (const [command, gesture] of moves) {
      expect(command).toMatchObject({
        type: "setInstrumentParam",
        channel,
        param,
      })
      expect(gesture).toEqual(expect.any(Number))
    }
    expect(new Set(moves.map(([, gesture]) => gesture)).size).toBe(1)
    expect(channelOf(channel).source.params).not.toEqual(original)
    expect(history().cursor).toBe(cursor + 1)
    await act(() => undo())
    await flush()
    expect(channelOf(channel).source.params).toEqual(original)
  })

  it("uses the existing instrument parameter target for generic control automation", async () => {
    const channel = await open("acidLine")
    const descriptor = instrumentDescriptor("acidLine")
    const param = paramIndex(descriptor, "accentAmount")
    fireEvent.contextMenu(
      screen.getByRole("slider", { name: descriptor.params[param].name }),
      { clientX: 20, clientY: 20 }
    )
    await userEvent
      .setup()
      .click(
        await screen.findByRole("menuitem", { name: "Create automation clip" })
      )
    await flush()
    expect(
      useProjectStore.getState().project.automations.at(-1)?.target
    ).toEqual({ type: "instrumentParam", channel, param })
  })

  it("edits Finger Bass tone through history with its six string/body controls", async () => {
    const channel = await open("fingerBass")
    const descriptor = instrumentDescriptor("fingerBass")
    const original = channelOf(channel).source.params
    const tone = descriptor.params[paramIndex(descriptor, "tone")]
    const control = screen.getByRole("slider", { name: tone.name })
    fireEvent.keyDown(control, { key: "End" })
    fireEvent.keyUp(control, { key: "End" })
    await flush()
    expect(channelOf(channel).source.params).not.toEqual(original)
    expect(control).toHaveAttribute("aria-valuenow", "1")
    await act(() => undo())
    expect(channelOf(channel).source.params).toEqual(original)
    expect(screen.getAllByRole("slider")).toHaveLength(6)
  })

  it("edits the single membrane's strike position and retains it through undo", async () => {
    const channel = await open("membrane")
    const original = channelOf(channel).source.params
    expect(screen.getByRole("img", { name: /Strike at 25 percent/ })).toBeVisible()
    const strike = screen.getByRole("slider", { name: "Strike Position" })
    fireEvent.keyDown(strike, { key: "End" })
    fireEvent.keyUp(strike, { key: "End" })
    await flush()
    expect(screen.getByRole("img", { name: /Strike at 95 percent/ })).toBeVisible()
    expect(screen.getByRole("slider", { name: "Damping" })).toBeVisible()
    expect(screen.getByRole("slider", { name: "Body" })).toBeVisible()
    await act(() => undo())
    expect(channelOf(channel).source.params).toEqual(original)
  })

  it("selects and edits the sixteenth synthesized membrane without changing pad one", async () => {
    const channel = await open("drumRack")
    const original = channelOf(channel).source.params
    await userEvent.setup().click(screen.getByRole("button", { name: "Pad 16 · MIDI 51" }))
    const strike = screen.getByRole("slider", { name: "Strike Position" })
    fireEvent.keyDown(strike, { key: "End" })
    fireEvent.keyUp(strike, { key: "End" })
    await flush()
    const edited = channelOf(channel).source.params
    if (original.type !== "drumRack" || edited.type !== "drumRack") throw new Error("wrong kind")
    expect(edited.pads[0]).toEqual(original.pads[0])
    expect(edited.pads[15].strikePosition).toBeCloseTo(0.95)
    await act(() => undo())
    expect(channelOf(channel).source.params).toEqual(original)
  })

  it("stores independent patches for arbitrary drum keys and undoes a key edit", async () => {
    const channel = await open("drumVoice")
    const key = screen.getByRole("spinbutton", { name: "Program MIDI key" })
    fireEvent.change(key, { target: { value: "127" } })
    const editor = screen.getByRole("region", { name: "MIDI 127 patch" })
    const pitch = within(editor).getByRole("slider", { name: "Pitch" })
    fireEvent.keyDown(pitch, { key: "End" })
    fireEvent.keyUp(pitch, { key: "End" })
    await flush()
    expect(channelOf(channel).source.asset).toMatchObject({ type: "drumKeys", patches: [{ key: 127, params: { pitchHz: 2000 } }] })
    fireEvent.change(key, { target: { value: "36" } })
    expect(within(screen.getByRole("region", { name: "MIDI 36 patch" })).getByRole("slider", { name: "Pitch" })).toHaveAttribute("aria-valuenow", "120")
    await act(() => undo())
    expect(channelOf(channel).source.asset).toBeUndefined()
  })

  it("shows Layer Kick sample editing and removes/restores immutable layer data", async () => {
    const channel = await open("layerKick")
    await act(async () => { await dispatch({ type: "setInstrumentAsset", channel, expected: channelOf(channel).source, asset: { type: "pcm", sampleRate: 48000, samples: [0,8192,0] } }) })
    expect(screen.getByText("3 mono frames at 48000 Hz")).toBeVisible()
    expect(screen.getByRole("slider", { name: "Sample Mix" })).toBeVisible()
    expect(screen.getByRole("slider", { name: "Pitch Drop" })).toBeVisible()
    await userEvent.setup().click(screen.getByRole("button", { name: "Remove sample layer" }))
    await flush()
    expect(channelOf(channel).source.asset).toBeUndefined()
    await act(() => undo())
    expect(channelOf(channel).source.asset?.type).toBe("pcm")
  })

  it("mounts both harmonic snapshot spectra and automatable morph with history", async () => {
    const channel=await open("snapshotMorph"), original=channelOf(channel).source.params
    expect(screen.getByRole("img",{name:"Snapshot A spectrum"})).toBeVisible()
    expect(screen.getByRole("img",{name:"Snapshot B spectrum"})).toBeVisible()
    expect(screen.getByRole("spinbutton",{name:"Snapshot A harmonic level"})).toBeVisible()
    const morph=screen.getByRole("slider",{name:"Morph"})
    fireEvent.keyDown(morph,{key:"End"});fireEvent.keyUp(morph,{key:"End"});await flush()
    expect(channelOf(channel).source.params).not.toEqual(original)
    await act(() => undo());expect(channelOf(channel).source.params).toEqual(original)
  })

  it("mounts subtractive harmonic controls and undoes a cutoff edit",async()=>{
    const channel=await open("harmonicFilter"),original=channelOf(channel).source.params
    expect(screen.getByRole("img",{name:"Harmonic low-pass response"})).toBeVisible()
    const cutoff=screen.getByRole("slider",{name:"Cutoff"});fireEvent.keyDown(cutoff,{key:"Home"});fireEvent.keyUp(cutoff,{key:"Home"});await flush()
    expect(channelOf(channel).source.params).not.toEqual(original);await act(()=>undo());expect(channelOf(channel).source.params).toEqual(original)
  })

  it("edits metallic shimmer with its partial-spacing display and history",async()=>{
    const channel=await open("metalVoice"),original=channelOf(channel).source.params
    expect(screen.getByRole("img",{name:"Inharmonic metallic partial spacing"})).toBeVisible()
    const shimmer=screen.getByRole("slider",{name:"Shimmer Depth"});fireEvent.keyDown(shimmer,{key:"End"});fireEvent.keyUp(shimmer,{key:"End"});await flush()
    expect(channelOf(channel).source.params).not.toEqual(original);await act(()=>undo());expect(channelOf(channel).source.params).toEqual(original)
  })

  it("mounts full-width number selection with only output as a scalar control",async()=>{
    await open("numberVoice")
    expect(screen.getByRole("spinbutton",{name:"Patch number"})).toHaveAttribute("max","4294967295")
    expect(screen.getByRole("button",{name:"Load numbered patch"})).toBeVisible()
    expect(screen.getAllByRole("slider")).toHaveLength(1)
    expect(screen.getByRole("slider",{name:"Level"})).toBeVisible()
  })

  it("keeps hosted plugin controls ahead of the built-in panel", async () => {
    const channel = await open("acidLine")
    const plugin: PluginBinding = {
      target: { type: "instrument", channel },
      format: "clap",
      path: "/plugins/test.clap",
      id: "test.plugin",
      name: "Hosted instrument",
      state: [],
      parameters: [],
    }
    act(() =>
      useProjectStore.setState((state) => ({
        project: { ...state.project, plugins: [plugin] },
      }))
    )
    await flush()
    expect(screen.getByRole("heading", { name: plugin.name })).toBeVisible()
    expect(screen.getByText(/CLAP Â· test.plugin/)).toBeVisible()
    expect(
      screen.queryByRole("group", {
        name: `${instrumentDescriptor("acidLine").name} settings`,
      })
    ).toBeNull()
    expect(document.querySelector("[data-slot=synth-editor]")).toBeNull()
  })
})
