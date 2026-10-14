import { afterEach, describe, expect, it } from "vitest"

import rustSource from "../../../../../crates/windfall-ipc/src/ui_messages.rs?raw"
import { catalogs, localizeBackendText, message, nativeMessage, setLanguage } from "."
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
    // Entries may be wrapped over several lines by the formatter.
    const declared = new Map([...table![1].matchAll(/\(\s*"([^"]+)",\s*&\[([^\]]*)\],?\s*\)/g)]
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

  /** A coded message as the backend writes it, values in name order. */
  const coded = (code: string, params: Record<string, string | number> = {}) =>
    `windfall-message:${JSON.stringify({ code, params: Object.fromEntries(Object.entries(params).sort(([a], [b]) => a.localeCompare(b))) })}`
  const term = (text: string) => coded("text.term", { text })

  it("says one of Windfall's own words in the language shown, and anything else as it is", () => {
    expect(nativeMessage(term("Cutoff"), "en")).toBe("Cutoff")
    expect(nativeMessage(term("Cutoff"), "es")).toBe(message("Cutoff" as MessageKey, {}, "es"))
    expect(nativeMessage(term("Somebody's Knob"), "es")).toBe("Somebody's Knob")
    // A term whose text is itself coded is put into words.
    expect(nativeMessage(term(coded("files.theTemplate")), "es")).toBe("La plantilla")
  })

  it("writes an id as it is and groups a count", () => {
    setLanguage("es")
    expect(localizeBackendText(coded("project.notFound", { id: 12345, kind: term("channel") })))
      .toContain(" 12345")
    expect(localizeBackendText(coded("flp.mixer.tooManyTracks", { count: 12345, max: 125 })))
      .toContain(new Intl.NumberFormat("es").format(12345))
    setLanguage("en")
  })

  it("ends a message sent as a sentence with one full stop", () => {
    const quality = coded("codec.vorbisQuality", { max: 10, min: -1, quality: 11 })
    setLanguage("es")
    expect(localizeBackendText(coded("text.sentence", { text: quality })))
      .toBe(`${localizeBackendText(quality)}.`)
    const ended = coded("archive.error.cancelled")
    expect(localizeBackendText(coded("text.sentence", { text: ended }))).toBe(localizeBackendText(ended))
    setLanguage("en")
  })

  it("puts every coded message of a list the backend joined into words, and capitalises one shown alone", () => {
    const list = [coded("samples.missing", { path: "C:/a.wav" }), "what the system said", coded("samples.missing", { path: "C:/b.wav" })].join("; ")
    const failed = coded("updates.editorsNotResumed", { errors: list })
    expect(nativeMessage(failed, "es")).toBe(
      "Los editores de plugins no se han podido reanudar tras cancelar la actualización: Falta la muestra: C:/a.wav; what the system said; Falta la muestra: C:/b.wav")
    setLanguage("es")
    expect(localizeBackendText(list)).toBe("Falta la muestra: C:/a.wav; what the system said; Falta la muestra: C:/b.wav")
    // A project error starts in lower case, as it reads inside another one.
    const damaged = coded("project.load.damaged", { problem: coded("project.check.noMaster") })
    expect(localizeBackendText(damaged)).toBe("El archivo del proyecto está dañado: el mezclador no tiene pista maestra")
    setLanguage("en")
    expect(localizeBackendText(damaged)).toBe("The project file is damaged: the mixer has no master track")
  })

  it("says a refusal of each family in Spanish: project validation, the FL import review, an edit, a desktop file and a history label", () => {
    // Project validation, with the owner a coded phrase of its own.
    const owner = coded("project.owner.mixerTrack", { id: 3 })
    const volume = coded("project.check.volume", { max: 4, owner, value: 2.5 })
    setLanguage("es")
    expect(localizeBackendText(coded("project.load.damaged", { problem: volume })))
      .toBe("El archivo del proyecto está dañado: la pista de mezclador 3 tiene un volumen de 2,5, fuera del intervalo de 0 a 4")
    // An edit refused by the command lowering.
    expect(localizeBackendText(coded("project.edit.trackFull", { max: 10, track: "Drums" })))
      .toBe("La pista de mezclador «Drums» está llena: admite como máximo 10 efectos")
    expect(localizeBackendText(coded("project.notFound", { id: 12, kind: term("channel") })))
      .toBe(`${message("channel" as MessageKey, {}, "es")} 12: no existe`.replace(/^./, (first) => first.toLocaleUpperCase("es")))
    // Lines of the FL import review, plural and with notes made of terms.
    expect(localizeBackendText(coded("flp.mixer.effectUnknown", { effect: "Fruity Blank", track: "Insert 3" })))
      .toBe("Pista de mezclador «Insert 3»: el efecto «Fruity Blank» no tiene equivalente en Windfall. Se ha dejado fuera de la cadena y se han conservado sus ajustes.")
    expect(localizeBackendText(coded("flp.mixer.tooManyTracks", { count: 1, max: 125 })))
      .toBe("El mezclador de Windfall tiene 125 pistas. Se ha omitido 1 más, y lo que sonaba en ella suena en la pista maestra.")
    expect(localizeBackendText(coded("flp.native.fieldOmitted", { device: "Fruity Limiter", field: "ceiling", value: 0.5 })))
      .toBe("Fruity Limiter.ceiling=0,5: omitido: campo público decodificado sin un control equivalente en el sustituto (se conserva el estado).")
    const notes = [term("its second response curve has no equivalent"), term("the slopes of its cut bands were not carried over")].join("; ")
    expect(localizeBackendText(coded("flp.mixer.becameEffect", { effect: "EQ", kind: term("Parametric EQ"), notes, track: "Master" })))
      .toBe(`Pista de mezclador «Master»: «EQ» se ha convertido en el efecto ${message("Parametric EQ" as MessageKey, {}, "es")} de Windfall: su segunda curva de respuesta no tiene equivalente; no se han trasladado las pendientes de sus bandas de corte.`)
    // A desktop refusal that names what was not done.
    const what = coded("files.fileNotOpened", { file: "song.windfall" })
    expect(localizeBackendText(coded("files.refusedEdited", { what })))
      .toBe("«song.windfall» no se ha abierto porque el proyecto se editó mientras tanto. Guarda o descarta esos cambios y vuelve a intentarlo.")
    setLanguage("en")
    expect(localizeBackendText(coded("files.refusedEdited", { what })))
      .toBe("\"song.windfall\" was not opened because the project was edited in the meantime. Save or discard those changes, then try again.")
  })
})
