import userEvent from "@testing-library/user-event"
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { EffectId, EffectKind, TrackId } from "@/bindings"
import { ValueContextMenus } from "@/components/value-context-menu"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { startTestApp, settle } from "@/test/harness"
import { EffectEditor } from "../effect-editor"
import { wav } from "./wav-fixture"
let stop: () => void, track: TrackId
let backend: Awaited<ReturnType<typeof startTestApp>>["backend"]
beforeEach(async () => { ({ stop, backend } = await startTestApp()); track = useProjectStore.getState().project.mixer.tracks[1].id })
afterEach(() => { stop(); vi.restoreAllMocks() })
function Editor({ effect }: { effect: EffectId }) {
  const slot = useProjectStore((s) => s.project.mixer.tracks.find((t) => t.id === track)?.effects.find((e) => e.id === effect))
  return slot ? <EffectEditor trackId={track} slot={slot} /> : null
}
async function open(kind: EffectKind) {
  const result = await dispatch({ type: "addEffect", track, kind }); if (!result) throw new Error("Add failed")
  const effect = result.created[0]; render(<ValueContextMenus><Editor effect={effect} /></ValueContextMenus>); await act(settle); return effect
}
function slot(effect: EffectId) { return useProjectStore.getState().project.mixer.tracks.find((t) => t.id === track)?.effects.find((e) => e.id === effect) }
describe("spectral mounted editors", () => {
  it("keeps the sounding IR and history when native publication refuses an imported WAV", async () => {
    const effect = await open("convolver")
    await act(() => dispatch({ type: "setConvolverImpulse", track, effect, impulse: {
      name: "Admitted IR", sampleRate: 48000, frames: [[1, 1], [0.5, 0.5]],
    } }))
    const before = await backend.documentSnapshot()
    const publication = vi.spyOn(backend, "dispatch").mockRejectedValueOnce(new Error("IR exceeds the host-rate preparation bound"))
    const file = new File([wav([0.25])], "Refused IR.wav", { type: "audio/wav" })
    Object.defineProperty(file, "arrayBuffer", { value: () => Promise.resolve(wav([0.25])) })
    fireEvent.change(screen.getByLabelText("Impulse response WAV"), { target: { files: [file] } })
    await waitFor(() => expect(screen.getByText("Impulse change was refused; the previous IR is retained")).toBeTruthy())
    expect(publication).toHaveBeenCalledWith({ type: "setConvolverImpulse", track, effect, impulse: {
      name: "Refused IR.wav", sampleRate: 48000, frames: [[0.25, 0.25]],
    } }, undefined)
    expect(await backend.documentSnapshot()).toEqual(before)
    expect(slot(effect)?.impulse?.name).toBe("Admitted IR")
  })
  it("repeated WAV imports retain exact undo redo and saved reopen payloads for native Session publication", async () => {
    const effect = await open("convolver")
    for (const gain of [0.25, 0.5, 0.75]) {
      const before = structuredClone(slot(effect)?.impulse)
      const file = new File([wav([gain, gain * 0.5])], "Own IR.wav", { type: "audio/wav" })
      Object.defineProperty(file, "arrayBuffer", { value: () => Promise.resolve(wav([gain, gain * 0.5])) })
      fireEvent.change(screen.getByLabelText("Impulse response WAV"), { target: { files: [file] } })
      await waitFor(() => expect(slot(effect)?.impulse?.frames).toEqual([[gain, gain], [gain * 0.5, gain * 0.5]]))
      expect(slot(effect)?.impulse).toMatchObject({ name: "Own IR.wav", sampleRate: 48000 })
      const after = structuredClone(slot(effect)?.impulse)
      await act(() => undo()); expect(slot(effect)?.impulse).toEqual(before)
      await act(() => redo()); expect(slot(effect)?.impulse).toEqual(after)
    }
    const saved = (await backend.documentSnapshot()).project
    const path = await backend.projectSave("/projects/p2-convolver.windfall")
    await act(async () => { await backend.projectOpen(path); await settle() })
    expect(useProjectStore.getState().project).toEqual(saved)
    expect(screen.getByText(/Own IR.wav: 2 stereo frames at 48000 Hz/)).toBeTruthy()
  })
  it("loads and clears an IR through undoable document edits", async () => {
    const effect = await open("convolver")
    const file = new File([wav([1, .5])], "My IR.wav", { type: "audio/wav" })
    Object.defineProperty(file, "arrayBuffer", { value: () => Promise.resolve(wav([1, .5])) })
    fireEvent.change(screen.getByLabelText("Impulse response WAV"), { target: { files: [file] } })
    await waitFor(() => expect(slot(effect)?.impulse?.frames).toEqual([[1, 1], [.5, .5]]))
    fireEvent.click(screen.getByRole("button", { name: "Clear impulse" })); await act(settle); expect(slot(effect)?.impulse).toBeFalsy()
    await act(() => undo()); expect(slot(effect)?.impulse?.name).toBe("My IR.wav")
  })
  it("discards a slow file read after a document edit and restores saved EQ controls on undo", async () => {
    const effect = await open("convolver")
    let finish: (buffer: ArrayBuffer) => void = () => { throw new Error("Read not started") }
    const file = new File([wav([1])], "Slow.wav")
    Object.defineProperty(file, "arrayBuffer", { value: () => new Promise<ArrayBuffer>((resolve) => { finish = resolve }) })
    fireEvent.change(screen.getByLabelText("Impulse response WAV"), { target: { files: [file] } })
    await act(() => dispatch({ type: "setEffectParam", track, effect, param: 0, value: 0.5 }))
    await act(async () => { finish(wav([1])); await settle() })
    expect(slot(effect)?.impulse).toBeFalsy()
    expect(screen.getByRole("alert").textContent).toMatch(/project changed/)
    const low = screen.getByRole("spinbutton", { name: "Low (<500 Hz) dB" })
    fireEvent.change(low, { target: { value: "6" } })
    fireEvent.click(screen.getByRole("button", { name: "Apply EQ impulse" })); await act(settle)
    fireEvent.change(screen.getByRole("spinbutton", { name: "Low (<500 Hz) dB" }), { target: { value: "-3" } })
    fireEvent.click(screen.getByRole("button", { name: "Apply EQ impulse" })); await act(settle)
    await act(() => undo())
    expect(screen.getByRole("spinbutton", { name: "Low (<500 Hz) dB" })).toHaveValue(6)
    expect(slot(effect)?.impulse?.eqGains).toEqual([6, 0, 0])
  })
  it("rejects invalid input and applies a generated linear-phase EQ", async () => {
    const effect = await open("convolver")
    const file = new File([new Uint8Array(8)], "Bad.wav")
    Object.defineProperty(file, "arrayBuffer", { value: () => Promise.resolve(new ArrayBuffer(8)) })
    fireEvent.change(screen.getByLabelText("Impulse response WAV"), { target: { files: [file] } })
    await waitFor(() => expect(screen.getByRole("alert").textContent).toMatch(/WAV/)); expect(slot(effect)?.impulse).toBeFalsy()
    fireEvent.click(screen.getByRole("button", { name: "Apply EQ impulse" })); await act(settle); expect(slot(effect)?.impulse?.frames).toHaveLength(1025)
  })
  it("exposes real carrier routing and the actual configured band layout", async () => {
    await open("vocoder"); expect(screen.getByRole("img", { name: /16-band filter bank/ })).toBeTruthy()
    expect(screen.getByText(/Mixer sidechain key: modulator/)).toBeTruthy()
    expect(document.querySelector('[data-param="synthCarrier"]')).toBeTruthy()
    expect(document.querySelector('[data-param="keyIsCarrier"]')).toBeTruthy()
    const effect = useProjectStore.getState().project.mixer.tracks.find((t) => t.id === track)?.effects[0].id
    if (effect === undefined) throw new Error("Vocoder missing")
    await act(() => dispatch({ type: "setEffectParam", track, effect, param: 0, value: 32 }))
    expect(screen.getByRole("img", { name: /32-band filter bank/ })).toBeTruthy()
  })
  it("mounts independent pitch/formant modes and scale/harmony controls", async () => {
    const effect = await open("pitchCorrect")
    expect(document.querySelector('[data-param="scale"]')).toBeTruthy(); expect(document.querySelector('[data-param="harmonyMix"]')).toBeTruthy()
    const source = useProjectStore.getState().project.channels[0]
    const user = userEvent.setup()
    await user.click(screen.getByRole("combobox", { name: "Harmony MIDI source" }))
    await user.click(screen.getByRole("option", { name: source.name })); await act(settle)
    expect(slot(effect)?.params).toMatchObject({ harmonyChannel: source.id })
  })
  it("mounts independent formant and processing-mode controls", async () => {
    await open("pitchShift"); for (const id of ["semitones", "formantSemitones", "mode", "mix"]) expect(document.querySelector(`[data-param="${id}"]`)).toBeTruthy()
  })
})
