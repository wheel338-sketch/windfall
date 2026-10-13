import { act, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it } from "vitest"

import { setLanguage } from "."
import { LocalizedDiagnostic, LocalizedLabel } from "./diagnostic"

afterEach(() => setLanguage("en"))

describe("retained native diagnostic toast contents", () => {
  it("keeps the code and retranslates its mounted title and diagnostic", () => {
    const source = 'windfall-message:{"code":"midi.preview.expired","params":{}}'
    render(<><h2><LocalizedLabel source="Import MIDI" /></h2><p><LocalizedDiagnostic source={source} /></p><p><LocalizedLabel source={{ key: "Fill every {value0} steps", values: { value0: 4 } }} /></p></>)
    expect(screen.getByText("Import MIDI")).toBeVisible()
    expect(screen.getByText("Fill every 4 steps")).toBeVisible()
    expect(screen.getByText("This MIDI preview has expired. Review the file again.")).toBeVisible()
    act(() => setLanguage("es"))
    expect(screen.getByText("Importar MIDI")).toBeVisible()
    expect(screen.getByText("Rellenar cada 4 pasos")).toBeVisible()
    expect(screen.getByText("Esta revisión MIDI ha caducado. Vuelve a revisar el archivo.")).toBeVisible()
    act(() => setLanguage("en"))
    expect(screen.getByText("This MIDI preview has expired. Review the file again.")).toBeVisible()
  })
})
