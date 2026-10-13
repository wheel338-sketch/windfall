import { act, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"

import { dynamicTitle, setLanguage } from "@/lib/i18n"
import { ContextActions, type ContextItem } from "./context-actions"

afterEach(() => setLanguage("en"))

describe("retained context menu locale", () => {
  it("retranslates open static and dynamic entries and their refusal reason", async () => {
    const run = vi.fn()
    const items: ContextItem[] = [
      { ...dynamicTitle("Fill every {value0} steps", { value0: 4 }), run },
      { ...dynamicTitle("Paste value", {}), disabled: true, reason: "Another unit", run },
      { title: "Save", run },
    ]
    render(<ContextActions items={items}><button>Fixture</button></ContextActions>)
    fireEvent.contextMenu(screen.getByRole("button", { name: "Fixture" }), { clientX: 40, clientY: 60 })
    expect(await screen.findByRole("menuitem", { name: "Fill every 4 steps" })).toBeVisible()
    act(() => setLanguage("es"))
    expect(screen.getByRole("menuitem", { name: "Rellenar cada 4 pasos" })).toBeVisible()
    const refused = screen.getByRole("menuitem", { name: /Pegar valor/ })
    expect(refused).toHaveAttribute("aria-disabled", "true")
    expect(within(refused).getByText("Unidad diferente")).toBeVisible()
    // A user-owned title must not change just because it matches a catalog key.
    expect(screen.getByRole("menuitem", { name: "Save" })).toBeVisible()
    expect(run).not.toHaveBeenCalled()
    act(() => setLanguage("en"))
    expect(screen.getByRole("menuitem", { name: "Fill every 4 steps" })).toBeVisible()
  })
})
