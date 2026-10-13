import { act, fireEvent, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import { LanguageSection } from "@/features/settings/language"
import { PanelFrame } from "@/features/layout/panel-frame"
import { TooltipProvider } from "@/components/ui/tooltip"
import { NotebookPanel } from "@/features/notebook/panel"
import { catalogs, connectLanguage, formatDate, formatNumber, LANGUAGE_STORAGE_KEY, message, setLanguage, translatedText, useLanguageStore } from "."
import type { MessageKey } from "./types"
import { sameAsEnglish } from "./invariants"

afterEach(() => { setLanguage("en"); localStorage.removeItem(LANGUAGE_STORAGE_KEY); vi.restoreAllMocks() })
describe("shell message catalogs", () => {
  it("translates notebook controls and preview labels while preserving authored content", () => {
    setLanguage("es")
    render(<NotebookPanel notebook={{ pages: [{ title: "User title", body: "<p>User note</p>", html: true }] }} onSave={async () => true} />)
    expect(screen.getByRole("button", { name: "Añadir página" })).toBeVisible()
    expect(screen.getByRole("button", { name: "Guardar cuaderno" })).toBeVisible()
    expect(screen.getByTitle("Vista HTML del cuaderno")).toHaveAttribute("sandbox", "")
    expect(screen.getByDisplayValue("User title")).toBeVisible()
    expect(screen.getByDisplayValue("<p>User note</p>")).toBeVisible()
  })
  it("has every key, plural form and interpolation placeholder in every catalog", () => {
    for (const catalog of Object.values(catalogs)) {
      expect(Object.keys(catalog).sort()).toEqual(Object.keys(catalogs.en).sort())
      for (const key of Object.keys(catalogs.en) as MessageKey[]) {
        const base = catalogs.en[key]; const translated = catalog[key]
        expect(typeof translated).toBe(typeof base)
        const forms = (entry: typeof base) => typeof entry === "string" ? [entry] : [entry.one, entry.other]
        forms(base).forEach((form, index) => {
          expect(forms(translated)[index].length).toBeGreaterThan(0)
          expect(forms(translated)[index].match(/\{\w+\}/g)?.sort()).toEqual(form.match(/\{\w+\}/g)?.sort())
        })
      }
    }
  })
  it("rejects untranslated English entries unless their equality has an explicit reason", () => {
    for (const [language, catalog] of Object.entries(catalogs)) {
      if (language === "en") continue
      for (const key of Object.keys(catalogs.en) as MessageKey[]) {
        const base = catalogs.en[key]
        const translated = catalog[key]
        const forms = (entry: typeof base) => typeof entry === "string" ? [entry] : [entry.one, entry.other]
        forms(base).forEach((form, index) => {
          if (forms(translated)[index] === form)
            expect(sameAsEnglish[key], `${language}: untranslated English for ${key}`).toBeTruthy()
        })
      }
    }
    // An obsolete exception must not become an unnoticed future escape hatch.
    for (const [key, reason] of Object.entries(sameAsEnglish)) {
      expect(Object.hasOwn(catalogs.en, key), key).toBe(true)
      expect(reason.length, key).toBeGreaterThan(10)
    }
  })
  it("formats plurals, numbers and dates using the selected locale", () => {
    expect(message("pages", { count: 1 }, "es")).toBe("1 página")
    expect(message("pages", { count: 2 }, "es")).toBe("2 páginas")
    expect(formatNumber(1.5, { minimumFractionDigits: 1 }, "es")).toBe("1,5")
    expect(formatDate(Date.UTC(2026, 9, 11), { timeZone: "UTC", year: "numeric", month: "2-digit", day: "2-digit" }, "es")).toBe("11/10/2026")
    expect(translatedText("User named track", "es")).toBe("User named track")
  })
  it("mounted language choice persists and immediately translates main panel headings", async () => {
    const user = userEvent.setup()
    render(<TooltipProvider><LanguageSection /><PanelFrame title="Mixer"><p>User text</p></PanelFrame></TooltipProvider>)
    await user.click(screen.getByRole("combobox", { name: "Interface language" }))
    await user.click(await screen.findByRole("option", { name: "Español" }))
    expect(screen.getByRole("heading", { name: "Mezclador" })).toBeVisible()
    expect(screen.getByRole("combobox", { name: "Idioma de la interfaz" })).toBeVisible()
    expect(localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("es")
    expect(document.documentElement.lang).toBe("es")
    expect(screen.getByText("User text")).toBeVisible()
  })
  it("rehydrates other-window language changes, handles invalid preferences and releases listener", () => {
    const stop = connectLanguage()
    localStorage.setItem(LANGUAGE_STORAGE_KEY, "es")
    const write = vi.spyOn(Storage.prototype, "setItem")
    act(() => fireEvent(window, new StorageEvent("storage", { key: LANGUAGE_STORAGE_KEY, newValue: "es" })))
    expect(useLanguageStore.getState().language).toBe("es")
    expect(write).not.toHaveBeenCalled()
    localStorage.setItem(LANGUAGE_STORAGE_KEY, "unsupported")
    act(() => fireEvent(window, new StorageEvent("storage", { key: LANGUAGE_STORAGE_KEY, newValue: "unsupported" })))
    expect(useLanguageStore.getState().language).toBe("en")
    stop()
    localStorage.setItem(LANGUAGE_STORAGE_KEY, "es")
    fireEvent(window, new StorageEvent("storage", { key: LANGUAGE_STORAGE_KEY }))
    expect(useLanguageStore.getState().language).toBe("en")
  })
})
