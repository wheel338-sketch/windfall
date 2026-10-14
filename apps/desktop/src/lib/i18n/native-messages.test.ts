import { afterEach, describe, expect, it } from "vitest"

import rustSource from "../../../../../crates/windfall-ipc/src/ui_messages.rs?raw"
import { catalogs, localizeBackendText, nativeMessage, setLanguage } from "."
import { nativeEn, nativeEs } from "./native-messages"
import type { MessageKey } from "./types"

afterEach(() => setLanguage("en"))

describe("native coded message localization", () => {
  it("has a catalog entry in every language for every Rust wire code", () => {
    const table = rustSource.match(/pub const UI_MESSAGE_CODES: &\[&str\] = &\[([\s\S]*?)\];/)
    expect(table).not.toBeNull()
    const codes = [...table![1].matchAll(/"([^"]+)"/g)].map((match) => match[1])
    expect(codes.length).toBeGreaterThan(30)
    expect(new Set(codes).size).toBe(codes.length)
    expect(codes.sort()).toEqual(Object.keys(nativeEn).filter((code) => !["native.messageUnknown", "native.messageInvalid"].includes(code)).sort())
    expect(Object.keys(nativeEs).sort()).toEqual(Object.keys(nativeEn).sort())
    for (const code of codes) for (const catalog of Object.values(catalogs)) {
      expect(Object.hasOwn(catalog, code), code).toBe(true)
      expect(catalog[code as MessageKey], code).toBeTruthy()
    }
  })

  it("shows every code with exactly the parameters Rust declares for it, in every language", () => {
    const table = rustSource.match(/pub const UI_MESSAGE_PARAMS: &\[\(&str, &\[&str\]\)\] = &\[([\s\S]*?)\n\];/)
    expect(table).not.toBeNull()
    const declared = new Map([...table![1].matchAll(/\("([^"]+)", &\[([^\]]*)\]\)/g)]
      .map((match) => [match[1], [...match[2].matchAll(/"([^"]+)"/g)].map((name) => name[1]).sort()] as const))
    expect(declared.size).toBeGreaterThan(80)
    expect([...declared.keys()].sort()).toEqual(Object.keys(nativeEn).filter((code) => !code.startsWith("native.")).sort())
    const shownWith = (form: string) => [...new Set([...form.matchAll(/\{(\w+)\}/g)].map((match) => match[1]))].sort()
    const wrong: string[] = []
    for (const [code, params] of declared) for (const [language, catalog] of [["en", nativeEn], ["es", nativeEs]] as const) {
      const entry: string | { one: string; other: string } = catalog[code as keyof typeof nativeEn]
      // Every form of a plural entry names the same parameters.
      for (const form of typeof entry === "string" ? [entry] : [entry.one, entry.other])
        if (shownWith(form).join() !== params.join()) wrong.push(`${language}: ${code}: ${shownWith(form).join()} instead of ${params.join()}`)
    }
    expect(wrong).toEqual([])
  })

  it("says a coded backend refusal in the interface language, with its cause as it came", () => {
    const refused = 'windfall-message:{"code":"templates.factoryName","params":{"name":"Empty"}}'
    expect(nativeMessage(refused, "en")).toBe('"Empty" is a template that comes with Windfall. Pick another name.')
    expect(nativeMessage(refused, "es")).toBe("«Empty» es una plantilla incluida con Windfall. Elige otro nombre.")
    // What the operating system said is shown as it came.
    const failed = 'windfall-message:{"code":"mixerPresets.openFailed","params":{"error":"Access is denied. (os error 5)"}}'
    expect(nativeMessage(failed, "es")).toBe("No se ha podido abrir el preajuste de mezclador: Access is denied. (os error 5)")
    // A cause that is a coded message itself is put into words too.
    const cause = 'windfall-message:{"code":"plugins.databaseReadFailed","params":{"error":"unexpected end of file"}}'
    const locked = `windfall-message:${JSON.stringify({ code: "plugins.databaseLocked", params: { error: cause, file: "C:/Users/a/plugins.json" } })}`
    expect(nativeMessage(locked, "es")).toBe(
      "No se ha podido leer la base de datos de plugins: unexpected end of file. Los favoritos y las categorías no se están guardando. " +
      "Repara o cambia el nombre de «C:/Users/a/plugins.json» y reinicia para volver a guardarlos.")
    expect(nativeMessage(locked, "en")).toContain("The plugin database could not be read: unexpected end of file. Favorites and categories")
    setLanguage("es")
    expect(localizeBackendText(refused)).toBe("«Empty» es una plantilla incluida con Windfall. Elige otro nombre.")
  })

  it("retranslates retained codes, plurals and numeric parameters without changing authored names", () => {
    const retained = 'windfall-message:{"code":"midi.import.markersNotImported","params":{"count":1234}}'
    expect(nativeMessage(retained, "en")).toBe("1,234 markers not imported")
    expect(nativeMessage(retained, "es")).toBe("No se han importado 1234 marcadores")
    const singular = 'windfall-message:{"code":"midi.import.markersNotImported","params":{"count":1}}'
    expect(nativeMessage(singular, "es")).toBe("No se ha importado 1 marcador")
    const named = 'windfall-message:{"code":"midi.import.drumsAsSynth","params":{"channel":"Kick / User name"}}'
    expect(nativeMessage(named, "es")).toContain("«Kick / User name»")
    setLanguage("es")
    expect(nativeMessage(retained)).toBe("No se han importado 1234 marcadores")
    setLanguage("en")
    expect(nativeMessage(retained)).toBe("1,234 markers not imported")
  })

  it("rejects malformed envelopes visibly and leaves ordinary user text outside the decoder", () => {
    expect(nativeMessage("User title", "es")).toBeNull()
    expect(nativeMessage("windfall-message:{", "es")).toBe("No se ha podido interpretar el mensaje nativo.")
    expect(nativeMessage('windfall-message:{"code":"midi.import.markersNotImported","params":{"count":{}}}', "es"))
      .toBe("No se ha podido interpretar el mensaje nativo.")
    expect(nativeMessage('windfall-message:{"code":"midi.import.markersNotImported","params":{}}', "es"))
      .toBe("No se ha podido interpretar el mensaje nativo.")
    expect(nativeMessage('windfall-message:{"code":"future.code","params":{}}', "es"))
      .toBe("Mensaje nativo desconocido (future.code).")
  })
})
