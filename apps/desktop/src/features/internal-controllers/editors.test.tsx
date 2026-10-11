// Mounted acceptance cases: written, not executed; regenerate bindings/WASM first.
import { act, fireEvent, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, afterEach, expect, it, vi } from "vitest"
import type { EffectId, EffectKind, TrackId, RealtimeFrame } from "@/bindings"
import { EffectEditor } from "@/features/effects/effect-editor"
import { ValueContextMenus } from "@/components/value-context-menu"
import { dispatch, undo, receivePatch, useProjectStore } from "@/lib/store/project"
import { settle, startTestApp } from "@/test/harness"
import { controllerEntries } from "@/features/controllers/menu"
import { addInternalLink, internalSources } from "./model"
import { DashboardEditor } from "./dashboard"
import type { MockBackend } from "@/lib/ipc/mock"
import { subscribeRealtime } from "@/lib/store/realtime"
import { useTransportStore } from "@/lib/store/transport"
import { useAutomationRecordStore, watchAutomationRecording } from "@/features/automation/record"
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }) }))
let stop: () => void
let appBackend: MockBackend
let track: TrackId
beforeEach(async () => {
  const app = await startTestApp(); stop = app.stop; appBackend = app.backend;
  track = useProjectStore.getState().project.mixer.tracks[1].id
})
afterEach(() => { stop(); vi.restoreAllMocks() })
function Bound({ id }: { id: EffectId }) {
  const slot = useProjectStore((s) => s.project.mixer.tracks.find((t) => t.id === track)?.effects.find((e) => e.id === id))
  return slot ? <EffectEditor trackId={track} slot={slot} /> : null
}
async function open(kind: EffectKind) {
  const result = await dispatch({ type: "addEffect", track, kind }); if (!result) throw new Error("Could not add controller")
  const id = result.created[0]
  render(<ValueContextMenus><Bound id={id} /></ValueContextMenus>);await act(settle);return id
}
function config(id: number) { return useProjectStore.getState().project.settings.internalControllers?.find((c) => c.controller === id) }
it.each([
  ["noteEnvelope", "Note envelope source"], ["keyboardSource", "Keyboard control source"],
  ["formulaSource", "Formula control source"], ["envelopeFollower", "Audio peak and LFO source"],
  ["voltageSource", "DC control source"],
] satisfies [EffectKind,string][])("mounts purpose-built %s editor", async (kind, label) => { await open(kind);expect(screen.getByLabelText(label)).toBeInTheDocument() })
it.each(["xyPad", "xyzPad"] satisfies EffectKind[])("pad %s moves both axes in one undo gesture", async (kind) => {
  const id = await open(kind);const pad = screen.getByRole("group", { name: kind === "xyPad" ? "XY control pad" : "XYZ control pad" })
  vi.spyOn(pad, "getBoundingClientRect").mockReturnValue(new DOMRect(0,0,200,100))
  const before = useProjectStore.getState().history.entries.length
  fireEvent.pointerDown(pad, { pointerId: 1, clientX: 150, clientY: 75 });fireEvent.pointerUp(pad, { pointerId: 1 });await act(settle)
  const params = useProjectStore.getState().project.mixer.tracks.find((t) => t.id === track)?.effects.find((e) => e.id === id)?.params
  expect(params && "x" in params ? params.x : null).toBe(0.75);expect(params && "y" in params ? params.y : null).toBe(0.25)
  expect(useProjectStore.getState().history.entries.length).toBe(before + 1)
  await act(undo);expect(useProjectStore.getState().history.cursor).toBe(before)
})
it("formula editor persists three-input expression and shows a refusal without overwriting it", async () => {
  const id = await open("formulaSource");const user = userEvent.setup();const input = screen.getByLabelText("Formula")
  await user.clear(input);await user.type(input,"a + b * c");await user.click(screen.getByRole("button",{name:"Apply formula"}));await act(settle)
  expect(config(id)?.expression).toBe("a + b * c")
  await user.clear(input);await user.type(input,"a +");await user.click(screen.getByRole("button",{name:"Apply formula"}));await act(settle)
  expect(config(id)?.expression).toBe("a + b * c");expect(screen.getByText(/Formula was refused/)).toBeInTheDocument()
})
it("envelope editor stores the actual drawn curve in one edit",async () => {
  const id = await open("noteEnvelope");const svg = screen.getByRole("img",{name:"Drawn note envelope"})
  vi.spyOn(svg,"getBoundingClientRect").mockReturnValue(new DOMRect(0,0,200,100))
  fireEvent.pointerDown(svg,{pointerId:2,clientX:100,clientY:0});fireEvent.pointerUp(svg,{pointerId:2});await act(settle)
  expect(config(id)?.envelope).toEqual([{position:0,value:0},{position:0.5,value:1},{position:1,value:0}])
  await act(undo);expect(config(id)?.envelope ?? []).toEqual([])
})
it("peak editor switches real detector mode and musical LFO sync",async () => {
  const id = await open("envelopeFollower");const user = userEvent.setup()
  await user.click(screen.getByRole("checkbox",{name:"RMS detector"}));await user.click(screen.getByRole("checkbox",{name:"Sync LFO to tempo"}));await act(settle)
  expect(config(id)?.rms).toBe(true);expect(config(id)?.syncBeats).toBe(1);expect(screen.getByLabelText("Beats per cycle")).toBeInTheDocument()
})
it("control surface starts blank and persists a configurable named widget",async () => {
  const id = await open("controlSurface");const user = userEvent.setup()
  await user.click(screen.getByRole("button",{name:"Add control"}));await act(settle)
  fireEvent.change(screen.getByLabelText("Control 1 style"),{target:{value:"button"}});await act(settle)
  expect(config(id)?.controls?.[0].kind).toBe("button");expect(screen.getByRole("button",{name:"Control 1"})).toBeInTheDocument()
})
it("voltage editor adds an explicit spare output route and reports the hardware gate",async () => {
  const id = await open("voltageSource");const user = userEvent.setup();await user.click(screen.getByRole("button",{name:"Add output route"}));await act(settle)
  expect(config(id)?.voltageRoutes).toEqual([{output:0,channel:2,zero:0,scale:1}]);expect(screen.getByText(/hardware voltage and voltage safety/)).toBeInTheDocument()
})
it("internal linking is offered in browser menus and ranges are editable",async () => {
  const id = await open("controlSurface");const target = {type:"trackPan",track} as const
  const choices = controllerEntries(target).flatMap((item) => typeof item === "object" && "dynamic" in item ? item.dynamic() : [])
  expect(choices).toContainEqual(expect.objectContaining({submenu:"Link to internal controller"}))
  expect(internalSources(useProjectStore.getState().project).some((s) => s.id === id)).toBe(true)
  await act(async () => { await addInternalLink(id,0,target) });await act(settle)
  fireEvent.change(screen.getByLabelText("min"),{target:{value:"0.8"}});fireEvent.blur(screen.getByLabelText("min"));await act(settle)
  expect(useProjectStore.getState().project.settings.controllerLinks?.[0].map.min).toBeCloseTo(0.8)
})
it("dashboard mounts configurable panel with real assignable MIDI controls",async () => {
  const result = await dispatch({type:"addChannel",instrument:"dashboard",name:"Panel"});if (!result) throw new Error("Could not add dashboard")
  const id = result.created[0]
  function Panel() {
    const channel = useProjectStore((s) => s.project.channels.find((c) => c.id === id))
    return channel?.source.type === "instrument" && channel.source.params.type === "dashboard" ? <DashboardEditor channel={id} params={channel.source.params} /> : null
  }
  render(<ValueContextMenus><Panel /></ValueContextMenus>);await act(settle)
  expect(screen.getByLabelText("Dashboard panel")).toBeInTheDocument();await userEvent.setup().click(screen.getByRole("button",{name:"Add control"}));await act(settle)
  expect(config(id)?.controls?.[0].output).toBe(0);expect(internalSources(useProjectStore.getState().project).find((s) => s.id === id)?.ports.length).toBe(8)
})

it.each(["xyPad", "xyzPad"] satisfies EffectKind[])("pad %s axis edits are recordable into real saved automation curves", async (kind) => {
  let push: (frame: RealtimeFrame) => void = () => { throw new Error("No realtime subscriber") }
  let pending: FrameRequestCallback[] = []
  vi.spyOn(appBackend,"subscribeRealtime").mockImplementation((handler) => { push = handler; return () => {} })
  vi.stubGlobal("requestAnimationFrame",(callback: FrameRequestCallback) => { pending.push(callback); return pending.length })
  vi.stubGlobal("cancelAnimationFrame",() => {})
  const stopFeed = subscribeRealtime(() => undefined)
  const id = await open(kind)
  const recorded: number[] = []
  for (const param of [0,1]) {
    const result = await appBackend.automate({type:"effectParam",track,effect:id,param})
    receivePatch(result.patch); recorded.push(result.created[0])
  }
  await act(settle)
  useTransportStore.setState({playing:true,mode:"song"})
  push({playing:true,tick:120,meters:[],cpu:0,xruns:0,voices:0,gainReductions:[],automated:recorded.map((automation) => ({automation,value:0.5})),audioClips:0,droppedClips:0})
  const due = pending; pending = []; act(() => { for (const callback of due) callback(0) })
  const stopRecording = watchAutomationRecording()
  useAutomationRecordStore.setState({recordArmed:true})
  try {
    const pad = screen.getByRole("group",{name:kind === "xyPad" ? "XY control pad" : "XYZ control pad"})
    fireEvent.keyDown(pad,{key:"ArrowRight"}); await act(settle)
    fireEvent.keyDown(pad,{key:"ArrowUp"}); await act(settle)
    for (const automation of recorded) {
      const curve = useProjectStore.getState().project.automations.find((a) => a.id === automation)
      expect(curve?.points).toContainEqual({tick:120,value:0.51,curve:0,hold:false})
    }
  } finally {
    useAutomationRecordStore.setState({recordArmed:false}); stopRecording(); stopFeed(); vi.unstubAllGlobals()
  }
})
it("pad right-click leaves values and undo history untouched",async () => {
  await open("xyPad"); const pad = screen.getByRole("group",{name:"XY control pad"})
  vi.spyOn(pad,"getBoundingClientRect").mockReturnValue(new DOMRect(0,0,200,100))
  const before = useProjectStore.getState().history.entries.length
  fireEvent.pointerDown(pad,{button:2,pointerId:1,clientX:190,clientY:90}); fireEvent.pointerUp(pad,{button:2,pointerId:1}); await act(settle)
  expect(useProjectStore.getState().history.entries.length).toBe(before)
})
