import { afterEach, describe, expect, it } from "vitest"

import rustSource from "../../../../../crates/windfall-ipc/src/ui_messages.rs?raw"
import { catalogs, nativeMessage, setLanguage } from "."
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
