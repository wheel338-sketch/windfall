import userEvent from "@testing-library/user-event"
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { EffectId, EffectKind, TrackId } from "@/bindings"
import { ValueContextMenus } from "@/components/value-context-menu"
import { dispatch, undo, useProjectStore } from "@/lib/store/project"
import { startTestApp, settle } from "@/test/harness"
import { EffectEditor } from "../effect-editor"
import { wav } from "./wav-fixture"
let stop: () => void, track: TrackId
beforeEach(async () => { ({ stop } = await startTestApp()); track = useProjectStore.getState().project.mixer.tracks[1].id })
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
  it("loads and clears an IR through undoable document edits", async () => {
    const effect = await open("convolver")
    const file = new File([wav([1, .5])], "My IR.wav", { type: "audio/wav" })
    Object.defineProperty(file, "arrayBuffer", { value: () => Promise.resolve(wav([1, .5])) })
    fireEvent.change(screen.getByLabelText("Impulse response WAV"), { target: { files: [file] } })
    await waitFor(() => expect(slot(effect)?.impulse?.frames).toEqual([[1, 1], [.5, .5]]))
    fireEvent.click(screen.getByRole("button", { name: "Clear impulse" })); await act(settle); expect(slot(effect)?.impulse).toBeFalsy()
    await act(() => undo()); expect(slot(effect)?.impulse?.name).toBe("My IR.wav")
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
