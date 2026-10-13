import { act, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"

import type { ParamInfo } from "@/bindings"
import { ParamControl, ParamGroup } from "@/features/params"
import { setLanguage } from "."

afterEach(() => setLanguage("en"))

describe("mounted shared parameter localization", () => {
  it("updates group headings, accessible names, numeric readouts and choices without editing values", () => {
    const number: ParamInfo = { id: "value", name: "Volume", kind: "float", unit: "none", scale: "linear", min: 0, max: 2, default: 1.25, choices: [] }
    const choice: ParamInfo = { id: "choice", name: "Mix", kind: "choice", unit: "none", scale: "linear", min: 0, max: 1, default: 0, choices: [{ value: "on", label: "On" }, { value: "off", label: "Off" }] }
    const change = vi.fn()
    render(<ParamGroup title="Effects"><ParamControl info={number} value={1.25} onValueChange={change} /><ParamControl info={choice} value={0} onValueChange={change} /></ParamGroup>)
    expect(screen.getByRole("heading", { name: "Effects" })).toBeVisible()
    expect(screen.getByRole("slider", { name: "Volume" })).toHaveAttribute("aria-valuetext", "1.25")
    expect(screen.getByRole("radio", { name: "On" })).toHaveAttribute("aria-checked", "true")
    act(() => setLanguage("es"))
    expect(screen.getByRole("heading", { name: "Efectos" })).toBeVisible()
    expect(screen.getByRole("slider", { name: "Volumen" })).toHaveAttribute("aria-valuetext", "1,25")
    expect(screen.getByRole("slider", { name: "Volumen" })).toHaveAttribute("aria-valuenow", "1.25")
    expect(screen.getByRole("radio", { name: "Activado" })).toHaveAttribute("aria-checked", "true")
    expect(screen.getByRole("radio", { name: "Desactivado" })).toHaveAttribute("aria-checked", "false")
    expect(change).not.toHaveBeenCalled()
  })
})
