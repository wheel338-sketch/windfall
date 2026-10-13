import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { setLanguage } from "@/lib/i18n"
import { useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { startTestApp } from "@/test/harness"
import { MidiDialogs } from "./dialogs"
import { midiFixture } from "./fixture"

let app: Awaited<ReturnType<typeof startTestApp>>
beforeEach(async () => {
  setLanguage("en")
  app = await startTestApp({
    midiFile: async () => ({ name: "authored.mid", bytes: midiFixture() }),
  })
})
afterEach(() => { app.stop(); setLanguage("en") })

function open(dialog: "midiImport" | "midiExport") {
  useUiStore.getState().openDialog(dialog)
  render(<MidiDialogs />)
}

async function prepare() {
  vi.spyOn(app.backend, "pickMidiFile").mockResolvedValue("fixture.mid")
  vi.spyOn(app.backend, "midiPreview").mockResolvedValue({
    token: 42,
    channels: ["Lead"],
    notes: 1,
    patterns: 1,
    clips: 1,
    length: 3840,
    adjustments: ['windfall-message:{"code":"midi.import.markersNotImported","params":{"count":2}}'],
  })
  open("midiImport")
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: "Choose MIDI file…" }))
  await screen.findByText(
    "1 channel, 1 pattern, 1 clip and 1 note will be added."
  )
  return user
}

describe("MIDI dialogs", () => {
  it("retranslates the mounted MIDI interior and counted preview while preserving authored names", async () => {
    await prepare()
    act(() => setLanguage("es"))
    expect(screen.getByRole("button", { name: "Elegir archivo MIDI…" })).toBeVisible()
    expect(screen.getByRole("checkbox", { name: "Importar el primer compás" })).toBeVisible()
    expect(screen.getByText("Se añadirán 1 canal, 1 patrón, 1 fragmento MIDI y 1 nota.")).toBeVisible()
    expect(screen.getByText("Lead")).toBeVisible()
    expect(screen.getByText("No se han importado 2 marcadores")).toBeVisible()
    expect(screen.getByRole("button", { name: "Importar" })).toBeEnabled()
    act(() => setLanguage("en"))
    expect(screen.getByText("1 channel, 1 pattern, 1 clip and 1 note will be added.")).toBeVisible()
    expect(screen.getByText("2 markers not imported")).toBeVisible()
  })
  it("retranslates a retained native failure while the failed review stays open", async () => {
    const before = useProjectStore.getState().project
    vi.spyOn(app.backend, "pickMidiFile").mockResolvedValue("broken.mid")
    vi.spyOn(app.backend, "midiPreview").mockRejectedValue('windfall-message:{"code":"midi.preview.expired","params":{}}')
    open("midiImport")
    await userEvent.setup().click(screen.getByRole("button", { name: "Choose MIDI file…" }))
    expect(await screen.findByRole("alert")).toHaveTextContent("This MIDI preview has expired. Review the file again.")
    act(() => setLanguage("es"))
    expect(screen.getByRole("alert")).toHaveTextContent("Esta revisión MIDI ha caducado. Vuelve a revisar el archivo.")
    expect(screen.getByRole("button", { name: "Importar" })).toBeDisabled()
    expect(useProjectStore.getState().project).toBe(before)
    act(() => setLanguage("en"))
    expect(screen.getByRole("alert")).toHaveTextContent("This MIDI preview has expired. Review the file again.")
  })
  it("localizes MIDI validation diagnostics and retains the failed review without document edits", async () => {
    act(() => setLanguage("es"))
    const before = useProjectStore.getState().project
    vi.spyOn(app.backend, "pickMidiFile").mockResolvedValue("broken.mid")
    vi.spyOn(app.backend, "midiPreview").mockRejectedValue(new Error("The MIDI track is truncated."))
    open("midiImport")
    await userEvent.setup().click(screen.getByRole("button", { name: "Elegir archivo MIDI…" }))
    expect(await screen.findByRole("alert")).toHaveTextContent("La pista MIDI está truncada.")
    expect(screen.getByRole("button", { name: "Importar" })).toBeDisabled()
    expect(useProjectStore.getState().project).toBe(before)
  })

  it("exports chosen expression routing and validates bend/spacing options", async () => {
    const exported = vi.spyOn(app.backend, "exportMidi").mockResolvedValue("/exports/phrase.mid")
    open("midiExport")
    const user = userEvent.setup()
    await user.click(screen.getByLabelText("Expression routing"))
    await user.click(screen.getByRole("option", { name: "MPE · one channel per note" }))
    const bend = screen.getByLabelText("Pitch-bend range (semitones; 0 = automatic)")
    await user.clear(bend)
    await user.type(bend, "97")
    expect(screen.getByRole("button", { name: "Choose file and export…" })).toBeDisabled()
    await user.clear(bend)
    await user.type(bend, "48")
    const spacing = screen.getByLabelText("Expression spacing (ticks; 0 = automatic)")
    await user.clear(spacing)
    await user.type(spacing, "30")
    await user.click(screen.getByRole("button", { name: "Choose file and export…" }))
    expect(exported).toHaveBeenCalledWith(expect.any(String), expect.objectContaining({ expression: { mode: "mpe", bendRange: 48, step: 30 } }))
  })

  it("reviews MIDI again when note-expression import is disabled", async () => {
    const user = await prepare()
    await user.click(screen.getByRole("checkbox", { name: "Import pitch bend and controllers as note expression" }))
    await waitFor(() => expect(app.backend.midiPreview).toHaveBeenLastCalledWith("fixture.mid", expect.objectContaining({ noteExpression: false })))
  })
  it("imports synthetic MIDI through the dialog and removes it with one undo", async () => {
    const before = useProjectStore.getState().project
    open("midiImport")
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Choose MIDI file…" }))
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Import" })).toBeEnabled()
    )
    await user.click(screen.getByRole("button", { name: "Import" }))
    await waitFor(() => expect(useUiStore.getState().dialog).toBeNull())
    expect(useProjectStore.getState().project.channels).toHaveLength(
      before.channels.length + 1
    )
    await app.backend.undo()
    expect({
      ...useProjectStore.getState().project,
      nextId: before.nextId,
    }).toEqual(before)
  })
  it("shows the append behavior and reports unsupported content before import", async () => {
    await prepare()
    expect(screen.getByText("2 markers not imported")).toBeVisible()
    expect(screen.getByRole("button", { name: "Import" })).toBeEnabled()
    expect(screen.getByText(/Append notes and arrangement/)).toBeVisible()
  })

  it("refreshes the reviewed plan when import options change", async () => {
    const user = await prepare()
    await user.click(
      screen.getByRole("checkbox", { name: "Use factory drum samples" })
    )
    await waitFor(() =>
      expect(app.backend.midiPreview).toHaveBeenLastCalledWith(
        "fixture.mid",
        expect.objectContaining({ factoryDrums: true })
      )
    )
  })

  it("releases a review on cancel without dispatching", async () => {
    const discard = vi.spyOn(app.backend, "midiDiscard")
    const imported = vi.spyOn(app.backend, "importMidi")
    const user = await prepare()
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    await waitFor(() => expect(discard).toHaveBeenCalledWith(42))
    expect(imported).not.toHaveBeenCalled()
  })

  it("disables import and preserves the project after a parse failure", async () => {
    const before = useProjectStore.getState().project
    vi.spyOn(app.backend, "pickMidiFile").mockResolvedValue("broken.mid")
    vi.spyOn(app.backend, "midiPreview").mockRejectedValue(
      new Error("The MIDI track is truncated.")
    )
    open("midiImport")
    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: "Choose MIDI file…" }))
    expect(await screen.findByRole("alert")).toHaveTextContent("truncated")
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled()
    expect(useProjectStore.getState().project).toBe(before)
  })

  it("shows an import refusal and leaves the review open", async () => {
    const user = await prepare()
    vi.spyOn(app.backend, "importMidi").mockRejectedValue(
      new Error("This MIDI preview has expired.")
    )
    await user.click(screen.getByRole("button", { name: "Import" }))
    expect(await screen.findByRole("alert")).toHaveTextContent("expired")
    expect(useUiStore.getState().dialog).toBe("midiImport")
  })

  it("cancelled export writes nothing and keeps its options open", async () => {
    vi.spyOn(app.backend, "pickMidiExportPath").mockResolvedValue(null)
    const exported = vi.spyOn(app.backend, "exportMidi")
    open("midiExport")
    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: "Choose file and export…" }))
    expect(exported).not.toHaveBeenCalled()
    expect(useUiStore.getState().dialog).toBe("midiExport")
    expect(screen.getByText(/This exports no audio/)).toBeVisible()
  })

  it("reports write failures and allows retry", async () => {
    vi.spyOn(app.backend, "exportMidi").mockRejectedValue(
      new Error("The file is locked.")
    )
    open("midiExport")
    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: "Choose file and export…" }))
    expect(await screen.findByRole("alert")).toHaveTextContent("locked")
    expect(
      screen.getByRole("button", { name: "Choose file and export…" })
    ).toBeEnabled()
  })
})
