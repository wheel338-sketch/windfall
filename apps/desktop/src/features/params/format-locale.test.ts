import { describe, expect, it } from "vitest"

import { EFFECT_KINDS, effectDescriptor, INSTRUMENT_KINDS, instrumentDescriptor, paramInfo } from "./descriptors"
import { formatParam, formatParamDisplay, parseParam, parseParamDisplay } from "./format"

describe("localized parameter readouts", () => {
  it("uses locale decimals and reads them back while accepting canonical technical input", () => {
    const cutoff = paramInfo(instrumentDescriptor("subtractiveSynth"), "filter.cutoffHz")
    expect(formatParamDisplay(cutoff, 1200, "es")).toBe("1,20 kHz")
    expect(parseParamDisplay(cutoff, "1,20 kHz", "es")).toBe(1200)
    expect(parseParamDisplay(cutoff, "1.20 kHz", "es")).toBe(1200)
    expect(parseParamDisplay(cutoff, "1.234,5 Hz", "es")).toBe(1234.5)
    expect(formatParam(cutoff, 1200)).toBe("1.20 kHz")
    const toggle = paramInfo(effectDescriptor("compressor"), "autoMakeup")
    expect(formatParamDisplay(toggle, 1, "es")).toBe("Activado")
    expect(parseParamDisplay(toggle, "Activado", "es")).toBe(1)
    expect(parseParamDisplay(toggle, "Desactivado", "es")).toBe(0)
  })

  it("preserves the canonical parsed value at every descriptor endpoint and default in both locales", () => {
    const descriptors = [...EFFECT_KINDS.map(effectDescriptor), ...INSTRUMENT_KINDS.map(instrumentDescriptor)]
    let cases = 0
    for (const descriptor of descriptors) for (const info of descriptor.params) {
      const values = info.kind === "choice" ? info.choices.map((_, index) => index) : [info.min, info.default, info.max]
      for (const value of values) for (const language of ["en", "es"] as const) {
        const canonical = formatParam(info, value)
        const displayed = formatParamDisplay(info, value, language)
        expect(parseParamDisplay(info, displayed, language), `${descriptor.name}/${info.id}/${language}: ${displayed}`)
          .toBe(parseParam(info, canonical))
        cases += 1
      }
    }
    expect(cases).toBeGreaterThan(10_000)
  })
})
