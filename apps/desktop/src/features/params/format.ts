import type { ParamInfo } from "@/bindings"
import {
  FADER_MAX_GAIN,
  faderTaper,
  formatDb,
  formatGain,
  formatMs,
  formatPan,
  parseGain,
  parseHz,
  parseMs,
  parseNumber,
  parsePan,
  parsePercent,
  PLAIN_NUMBER,
  powerScale,
  type ValueScaleOption,
} from "@/components/audio"

import { clampParam } from "./access"
import { formatNumber, locales, translatedText, useLanguageStore, type Language } from "@/lib/i18n"

const MINUS = "−"
const INFINITY_TEXT = "∞"

/** A number with the kit's minus sign, and a plus sign when asked for. */
function signed(value: number, digits: number, plus: boolean): string {
  const fixed = Math.abs(value).toFixed(digits)
  if (Number(fixed) === 0) return fixed
  if (value < 0) return `${MINUS}${fixed}`
  return plus ? `+${fixed}` : fixed
}

function hertz(hz: number): string {
  const size = Math.abs(hz)
  if (size >= 10_000) return `${signed(hz / 1000, 1, false)} kHz`
  if (size >= 1000) return `${signed(hz / 1000, 2, false)} kHz`
  if (size >= 100) return `${signed(hz, 0, false)} Hz`
  return `${signed(hz, size >= 10 ? 1 : 2, false)} Hz`
}

function milliseconds(ms: number, plus: boolean): string {
  const size = Math.abs(ms)
  // The kit rounds to a tenth, which would show a 0.05 ms attack as 0.1.
  const text =
    size > 0 && size < 1 ? `${signed(ms, 2, false)} ms` : formatMs(ms)
  // A fractional delay can round to a whole millisecond. Keep the readout
  // identical when that displayed value is typed back into the control.
  const readout = text.replace(/\.0 ms$/, " ms")
  return plus && ms > 0 ? `+${readout}` : readout
}

function ratio(info: ParamInfo, value: number): string {
  // The core treats the top of a ratio's range as infinity to one.
  if (value >= info.max) return `${INFINITY_TEXT}:1`
  const text = value.toFixed(value >= 10 ? 0 : 1).replace(/\.0$/, "")
  return `${text}:1`
}

function plain(info: ParamInfo, value: number): string {
  if (info.kind === "integer") return signed(Math.round(value), 0, false)
  const size = Math.abs(value)
  return signed(value, size >= 100 ? 0 : size >= 10 ? 1 : 2, false)
}

/**
 * The kind of number a setting holds, for copying its value to another
 * control: its unit, and for a setting without one a bare number. A level
 * copied from a threshold then pastes into a make-up gain and not into a
 * cutoff.
 */
export function paramUnitKind(info: Pick<ParamInfo, "unit">): string {
  return info.unit === "none" ? PLAIN_NUMBER : info.unit
}

/**
 * A setting's value as text with its unit: "−18.0 dB", "1.20 kHz", "250 ms",
 * "70%", "+7 st", "4:1", "L30", "On", "Low-pass". `value` is the number
 * `readParam` returns.
 */
export function formatParam(info: ParamInfo, value: number): string {
  if (info.kind === "toggle") return value >= 0.5 ? "On" : "Off"
  if (info.kind === "choice") {
    return info.choices[clampParam(info, value)]?.label ?? ""
  }
  // A range that goes below zero is an offset, so its readout shows "+".
  const offset = info.min < 0
  switch (info.unit) {
    case "decibels":
      return offset ? formatDb(value, 1) : `${signed(value, 1, false)} dB`
    case "gain":
      return formatGain(value, 1)
    case "pan":
      return formatPan(value)
    case "hertz":
      return hertz(value)
    case "milliseconds":
      return milliseconds(value, offset)
    case "seconds":
      return milliseconds(value * 1000, offset)
    case "fraction":
      return `${signed(value * 100, 0, offset)}%`
    case "semitones":
      return `${signed(value, info.kind === "integer" ? 0 : 2, offset)} st`
    case "cents":
      return `${signed(value, info.kind === "integer" ? 0 : 1, offset)} ct`
    case "octaves":
      return `${signed(value, 2, offset)} oct`
    case "ratio":
      return ratio(info, value)
    case "none":
      return plain(info, value)
  }
}

/** Localized readout; `formatParam` remains the canonical interoperability spelling. */
export function formatParamDisplay(info: ParamInfo, value: number, language = useLanguageStore.getState().language): string {
  const canonical = formatParam(info, value)
  if (info.kind === "toggle" || info.kind === "choice") return translatedText(canonical, language)
  return canonical.replace(/\d+(?:\.\d+)?/g, (number) => {
    const digits = number.split(".")[1]?.length ?? 0
    return formatNumber(Number(number), { useGrouping: false, minimumFractionDigits: digits, maximumFractionDigits: digits }, language)
  })
}

/** Accept the displayed locale and the existing canonical technical input. */
export function parseParamDisplay(info: ParamInfo, text: string, language: Language = useLanguageStore.getState().language): number | null {
  const word = text.trim().toLowerCase()
  if (info.kind === "toggle") {
    if (word === translatedText("On", language).toLowerCase()) return 1
    if (word === translatedText("Off", language).toLowerCase()) return 0
  }
  if (info.kind === "choice") {
    const index = info.choices.findIndex((choice) => translatedText(choice.label, language).toLowerCase() === word)
    if (index >= 0) return index
  }
  const parts = new Intl.NumberFormat(locales[language]).formatToParts(12345.6)
  const decimal = parts.find((part) => part.type === "decimal")?.value ?? "."
  const group = parts.find((part) => part.type === "group")?.value
  let canonical = text
  // Canonical decimal dots stay usable in every locale. A grouping separator
  // is removed only when the text explicitly includes the locale's decimal.
  if (decimal !== "." && canonical.includes(decimal)) {
    if (group) canonical = canonical.replaceAll(group, "")
    canonical = canonical.replaceAll(decimal, ".")
  }
  return parseParam(info, canonical)
}

const ON = /^(on|yes|true|1)$/
const OFF = /^(off|no|false|0)$/

function parseChoice(info: ParamInfo, text: string): number | null {
  const wanted = text.trim().toLowerCase()
  if (wanted === "") return null
  const names = info.choices.map((choice) => [
    choice.label.toLowerCase(),
    choice.value.toLowerCase(),
  ])
  const exact = names.findIndex((pair) => pair.includes(wanted))
  if (exact >= 0) return exact
  const starts = names.findIndex((pair) =>
    pair.some((name) => name.startsWith(wanted))
  )
  return starts >= 0 ? starts : null
}

function parseUnit(info: ParamInfo, text: string): number | null {
  switch (info.unit) {
    case "gain":
      return parseGain(text)
    case "pan":
      return parsePan(text)
    case "hertz":
      return parseHz(text)
    case "milliseconds":
      return parseMs(text)
    case "seconds": {
      // A bare number is seconds; "250 ms" is still understood.
      const ms = parseMs(/[a-z]/i.test(text) ? text : `${text} s`)
      return ms === null ? null : ms / 1000
    }
    case "fraction":
      return parsePercent(text)
    case "ratio":
    case "decibels":
    case "semitones":
    case "cents":
    case "octaves":
    case "none":
      return parseNumber(text)
  }
}

/**
 * Reads typed text as a value of the setting, in the form `formatParam`
 * prints and looser ones ("-6", "1.2k", "1.5 s", "50", "inf", "saw"). The
 * result is inside the setting's range. Null when the text cannot be read.
 */
export function parseParam(info: ParamInfo, text: string): number | null {
  if (info.kind === "toggle") {
    const word = text.trim().toLowerCase()
    return ON.test(word) ? 1 : OFF.test(word) ? 0 : null
  }
  if (info.kind === "choice") return parseChoice(info, text)
  const parsed = parseUnit(info, text)
  if (parsed === null || Number.isNaN(parsed)) return null
  return clampParam(info, parsed)
}

/** The scale of a time that starts at zero and reaches a second or more. */
const LONG_TIME_SCALE = powerScale(3)

/**
 * How a knob spreads a setting's range over its travel. Logarithmic settings
 * get equal travel per octave. A linear gain that covers the fader's range
 * gets the fader's taper. A time that starts at 0 cannot be logarithmic, so
 * one that runs to a second or more gets a curve that leaves room for its
 * first milliseconds.
 */
export function paramScale(info: ParamInfo): ValueScaleOption {
  if (info.kind !== "float") return "linear"
  if (info.scale === "logarithmic" && info.min > 0) return "log"
  if (info.unit === "gain" && info.min === 0 && info.max === FADER_MAX_GAIN) {
    return faderTaper
  }
  const second =
    info.unit === "milliseconds" ? 1000 : info.unit === "seconds" ? 1 : null
  if (second !== null && info.min === 0 && info.max >= second) {
    return LONG_TIME_SCALE
  }
  return "linear"
}

/** Whether a setting is an offset around zero, drawn from the middle out. */
export function paramIsBipolar(info: ParamInfo): boolean {
  if (info.kind === "toggle" || info.kind === "choice") return false
  return info.unit === "pan" || (info.min < 0 && info.min === -info.max)
}
