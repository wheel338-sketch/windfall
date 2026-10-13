import { act, render, screen } from "@testing-library/react"
import { isValidElement } from "react"
import { toast } from "sonner"
import { afterEach, describe, expect, it, vi } from "vitest"

import { setLanguage } from "@/lib/i18n"
import { reportError } from "./errors"

vi.mock("sonner", () => ({ toast: { error: vi.fn() } }))
afterEach(() => { setLanguage("en"); vi.clearAllMocks() })

describe("coded native action error presentation", () => {
  it("keeps code and title parameters in the emitted toast so a locale change updates both", () => {
    reportError('windfall-message:{"code":"midi.preview.expired","params":{}}', {
      key: "Fill every {value0} steps", values: { value0: 4 },
    })
    const call = vi.mocked(toast.error).mock.calls.at(-1)
    if (!call) throw new Error("Expected a toast")
    const [title, options] = call
    const description = options?.description
    if (!isValidElement(title) || !isValidElement(description)) throw new Error("Expected reactive toast contents")
    render(<><h2>{title}</h2><p>{description}</p></>)
    expect(screen.getByText("Fill every 4 steps")).toBeVisible()
    expect(screen.getByText("This MIDI preview has expired. Review the file again.")).toBeVisible()
    act(() => setLanguage("es"))
    expect(screen.getByText("Rellenar cada 4 pasos")).toBeVisible()
    expect(screen.getByText("Esta revisión MIDI ha caducado. Vuelve a revisar el archivo.")).toBeVisible()
    expect(toast.error).toHaveBeenCalledTimes(1)
  })
})
