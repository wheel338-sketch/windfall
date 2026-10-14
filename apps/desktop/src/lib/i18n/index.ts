import { create } from "zustand"
import { en } from "./en"
import { es } from "./es"
import { nativeEn } from "./native-messages"
import { actionMessages, dynamicActionMessage, type DynamicActionMessage } from "./action-messages"
import type { Catalog, Language, Message, MessageKey } from "./types"
export type { Language, MessageKey } from "./types"
export const catalogs: Record<Language, Catalog> = { en, es }
export const LANGUAGE_STORAGE_KEY = "windfall.language"
export const locales = { en: "en-US", es: "es-ES" } as const
export function languageOf(value: unknown): Language { return value === "es" ? "es" : "en" }
function load(): Language {
  try { return languageOf(localStorage.getItem(LANGUAGE_STORAGE_KEY)) }
  catch { return "en" }
}
export const useLanguageStore = create(() => ({ language: load() }))
let reading: Language | null = null
/** The language text is made in now: the interface language, unless something is being read in another. */
function currentLanguage(): Language { return reading ?? useLanguageStore.getState().language }
/**
 * Runs `read` with every message it makes in `language`. A kept title holds
 * the way to make a word instead of the word, and this makes it again in the
 * language the title is shown in, or in English for the registry.
 */
export function inLanguage<T>(language: Language, read: () => T): T {
  const before = reading
  reading = language
  try { return read() }
  finally { reading = before }
}
export function setLanguage(language: Language) {
  useLanguageStore.setState({ language })
  document.documentElement.lang = language
  try { localStorage.setItem(LANGUAGE_STORAGE_KEY, language) } catch { /* Session choice remains usable. */ }
}
/** One listener per window, released with its shell. Storage rehydration never echoes a write. */
export function connectLanguage(): () => void {
  document.documentElement.lang = currentLanguage()
  const receive = (event: StorageEvent) => {
    if (event.key !== LANGUAGE_STORAGE_KEY && event.key !== null) return
    const language = load()
    useLanguageStore.setState({ language })
    document.documentElement.lang = language
  }
  window.addEventListener("storage", receive)
  return () => window.removeEventListener("storage", receive)
}
export function formatNumber(value: number, options?: Intl.NumberFormatOptions, language = currentLanguage()): string {
  return new Intl.NumberFormat(locales[language], options).format(value)
}
export function formatDate(value: Date | number, options?: Intl.DateTimeFormatOptions, language = currentLanguage()): string {
  return new Intl.DateTimeFormat(locales[language], options).format(value)
}
/** A number inside a sentence: the locale's decimal mark, with no grouping and no rounding. */
export function plainNumber(value: number, language = currentLanguage()): string {
  return Number.isFinite(value) ? formatNumber(value, { useGrouping: false, maximumFractionDigits: 20 }, language) : String(value)
}
export function message(key: MessageKey, values: Record<string, string | number> = {}, language = currentLanguage()): string {
  const entry = catalogs[language][key]
  const count = typeof values.count === "number" && Number.isFinite(values.count) ? values.count : 0
  const template = typeof entry === "string" ? entry : entry[new Intl.PluralRules(locales[language]).select(count) === "one" ? "one" : "other"]
  return template.replace(/\{(\w+)\}/g, (placeholder, name: string) => {
    const value = values[name]
    return value === undefined ? placeholder : typeof value === "number" ? formatNumber(value, undefined, language) : value
  })
}
/** Transitional display adapter for registry/toolbar labels. Never use on project/user text. */
export function translatedText(value: string, language = currentLanguage()): string {
  return Object.hasOwn(en, value) ? message(value as MessageKey, {}, language) : value
}
export function useTranslation() {
  const language = useLanguageStore((state) => state.language)
  return (value: string) => translatedText(value, language)
}
export function useActionTitle(id: string, fallback: string): string {
  const language = useLanguageStore((state) => state.language)
  return actionTitle(id, fallback, language)
}
export function actionTitle(id: string, fallback: string, language = currentLanguage()): string {
  const dynamic = dynamicActionMessage(id)
  if (dynamic) return dynamicMessage(dynamic, language)
  const key = actionMessages[id]
  return key ? message(key, {}, language) : id.startsWith("file.openRecent.") ? fallback : translatedText(fallback, language)
}

/** English compatibility title plus bounded registry-owned translation metadata. */
export function dynamicTitle(key: MessageKey, values: DynamicActionMessage["values"], translate?: DynamicActionMessage["translate"]) {
  const titleMessage: DynamicActionMessage = translate ? { key, values, translate } : { key, values }
  return { title: dynamicMessage(titleMessage, "en"), titleMessage }
}

/** A kept message in the language it is shown in, with the values it marks translated too. */
export function dynamicMessage(source: DynamicActionMessage, language = currentLanguage()): string {
  const values: Record<string, string | number> = {}
  // A value kept as a way to make it is made now, in the language shown.
  for (const [name, value] of Object.entries(source.values))
    values[name] = typeof value === "function" ? inLanguage(language, value) : value
  for (const [name, form] of Object.entries(source.translate ?? {})) {
    const value = values[name]
    if (typeof value !== "string") continue
    const translated = translatedText(value, language)
    values[name] = form === "lower" ? translated.toLocaleLowerCase(locales[language]) : translated
  }
  return message(source.key, values, language)
}

export function localizeBackendText(text: string): string {
  // One coded message, unless it is the first of several joined together.
  const end = text.startsWith(NATIVE_MESSAGE_PREFIX) ? jsonObjectEnd(text, NATIVE_MESSAGE_PREFIX.length) : -1
  const native = end < 0 || end === text.length ? nativeMessage(text) : null
  // A message shown on its own starts with a capital, as the backend's
  // English sentences always did; one inside another keeps its case.
  if (native !== null) return native.charAt(0).toLocaleUpperCase(locales[currentLanguage()]) + native.slice(1)
  // Lines of the backend joined together, some of them coded.
  if (text.includes(NATIVE_MESSAGE_PREFIX)) return embeddedMessages(text, currentLanguage())
  if (Object.hasOwn(en, text)) return translatedText(text)
  // A sentence of the backend's own that reaches here in lower case.
  const capital = text.charAt(0).toUpperCase() + text.slice(1)
  if (capital !== text && Object.hasOwn(en, capital)) return translatedText(capital)
  const prefixes = [
    ["Installation failed. Keep the current installation and retry. ", "Installation failed. Keep the current installation and retry. {detail}"],
    ["Download or signature verification failed. Nothing was installed. ", "Download or signature verification failed. Nothing was installed. {detail}"],
    ["Update check failed; retry when online. ", "Update check failed; retry when online. {detail}"],
  ] as const
  for (const [prefix,key] of prefixes) if (text.startsWith(prefix)) return message(key,{detail:text.slice(prefix.length)})
  const language = currentLanguage()
  return language === "en" || alreadyLocalized(text, language) ? text : message("Native diagnostic: {detail}", { detail: text })
}

const renderings = new Map<Language, { fixed: Set<string>; patterns: RegExp[] }>()
/**
 * Whether text is this language's rendering of a catalog entry. A failure the
 * app raises itself is translated where it is raised, so it arrives here
 * already in the interface language and is not an opaque native diagnostic.
 */
function alreadyLocalized(text: string, language: Language): boolean {
  let known = renderings.get(language)
  if (!known) {
    known = { fixed: new Set(), patterns: [] }
    for (const entry of Object.values<Message>(catalogs[language]))
      for (const form of typeof entry === "string" ? [entry] : [entry.one, entry.other]) {
        if (!form.includes("{")) { known.fixed.add(form); continue }
        // A template that is nearly all parameters would match any text.
        if (form.replace(/\{\w+\}/g, "").trim().length < 8) continue
        const literal = form.split(/\{\w+\}/).map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
        known.patterns.push(new RegExp(`^${literal.join("[\\s\\S]*")}$`))
      }
    renderings.set(language, known)
  }
  return known.fixed.has(text) || known.patterns.some((pattern) => pattern.test(text))
}

const HISTORY_LABEL_PREFIX = "windfall-label:"

/**
 * A history label as it is kept: the catalog key, with its values when it
 * has any, and never a translation. The history is read long after the edit
 * was made and possibly in another language; [`historyLabelText`] makes the
 * words then. A label without values is its English text, which is its key,
 * so the history of a project stays readable to anything that shows it raw.
 */
export function historyLabel(key: MessageKey, values: Record<string, string | number> = {}): string {
  return Object.keys(values).length === 0 ? key : `${HISTORY_LABEL_PREFIX}${JSON.stringify({ key, values })}`
}

/**
 * The words of a history label in the language it is shown in. A label the
 * frontend kept carries its key and values; a label the backend wrote is
 * its English text, which is a catalog key, or a coded message, or one of
 * the two it makes from a setting's name. Anything else, such as a label
 * from an older version or a name somebody typed, is shown as it is.
 */
export function historyLabelText(label: string, language = currentLanguage()): string {
  if (label.startsWith(HISTORY_LABEL_PREFIX)) {
    try {
      const kept: unknown = JSON.parse(label.slice(HISTORY_LABEL_PREFIX.length))
      if (kept && typeof kept === "object" && "key" in kept && typeof kept.key === "string" && Object.hasOwn(en, kept.key) &&
        "values" in kept && kept.values && typeof kept.values === "object" && !Array.isArray(kept.values)) {
        const entry = catalogs[language][kept.key as MessageKey]
        const values = kept.values as Record<string, unknown>
        const count = typeof values.count === "number" && Number.isFinite(values.count) ? values.count : 0
        const template = typeof entry === "string" ? entry : entry[new Intl.PluralRules(locales[language]).select(count) === "one" ? "one" : "other"]
        // Numbers inside a label are written plainly, as the labels made before this were.
        return template.replace(/\{(\w+)\}/g, (placeholder, name: string) => {
          const value = values[name]
          return typeof value === "number" && Number.isFinite(value) ? plainNumber(value, language) : typeof value === "string" ? value : placeholder
        })
      }
    } catch { /* Shown as it is. */ }
    return label
  }
  const native = nativeMessage(label, language)
  if (native !== null) return native
  if (Object.hasOwn(en, label)) return message(label as MessageKey, {}, language)
  // The backend names an edit of one setting after the setting.
  const script = /^Change script (.+)$/.exec(label)
  if (script) return message("Change script {name}", { name: translatedText(script[1], language) }, language)
  const setting = /^Change (.+)$/.exec(label)
  if (setting) return message("Change {name}", { name: translatedText(setting[1], language) }, language)
  return label
}

const NATIVE_MESSAGE_PREFIX = "windfall-message:"

/** The index just after the JSON object that starts at `start`, or -1. */
function jsonObjectEnd(text: string, start: number): number {
  if (text[start] !== "{") return -1
  let depth = 0
  for (let index = start; index < text.length; index++) {
    const c = text[index]
    if (c === "\"") {
      for (index++; index < text.length && text[index] !== "\""; index++) if (text[index] === "\\") index++
      continue
    }
    if (c === "{") depth++
    else if (c === "}" && --depth === 0) return index + 1
  }
  return -1
}

/**
 * `text` with every coded message in it put into words: a cause, or a list
 * of messages joined by the backend. Everything around them stays as it is.
 */
function embeddedMessages(text: string, language: Language): string {
  let out = ""
  let from = 0
  for (;;) {
    const at = text.indexOf(NATIVE_MESSAGE_PREFIX, from)
    const end = at < 0 ? -1 : jsonObjectEnd(text, at + NATIVE_MESSAGE_PREFIX.length)
    if (end < 0) return out + text.slice(from)
    out += text.slice(from, at) + (nativeMessage(text.slice(at, end), language) ?? text.slice(at, end))
    from = end
  }
}

/** The values of a coded message that hold a cause: never a name somebody typed. */
const CAUSES = new Set(["error", "errors", "problem", "reason", "detail"])
/** The values that number a thing rather than count it: written as they are, never grouped. */
const IDENTIFIERS = new Set(["id", "handle", "channel", "clip", "effect", "output", "pattern", "track"])

/**
 * `text` in the language shown when it is one of Windfall's own words or
 * sentences, and as it is otherwise. A sentence the backend sends on its own
 * starts with a capital and is a catalog key so; inside another message it
 * may start in lower case, and keeps its case.
 */
function ownSentence(text: string, language: Language): string {
  if (Object.hasOwn(en, text)) return message(text as MessageKey, {}, language)
  const capital = text.charAt(0).toUpperCase() + text.slice(1)
  if (capital !== text && Object.hasOwn(en, capital)) {
    const words = message(capital as MessageKey, {}, language)
    return words.charAt(0).toLocaleLowerCase(locales[language]) + words.slice(1)
  }
  return embeddedMessages(text, language)
}

/** Decode only the reserved native envelope, leaving authored text untouched. */
export function nativeMessage(text: string, language = currentLanguage()): string | null {
  if (!text.startsWith(NATIVE_MESSAGE_PREFIX)) return null
  try {
    const value: unknown = JSON.parse(text.slice(NATIVE_MESSAGE_PREFIX.length))
    if (!value || typeof value !== "object" || !("code" in value) ||
      typeof value.code !== "string" || !("params" in value) ||
      !value.params || typeof value.params !== "object" || Array.isArray(value.params))
      return message("native.messageInvalid", {}, language)
    if (!Object.hasOwn(nativeEn, value.code)) return message("native.messageUnknown", { code: value.code }, language)
    const params: Record<string, string | number> = {}
    for (const [key, parameter] of Object.entries(value.params)) {
      if (typeof parameter !== "string" && (typeof parameter !== "number" || !Number.isFinite(parameter)))
        return message("native.messageInvalid", {}, language)
      // A cause that is a coded message itself, or a list with coded
      // messages in it, is put into words as well; a cause that is one of
      // Windfall's own sentences is said in the language shown; anything
      // else, such as what the operating system said, stays as it is.
      params[key] = typeof parameter !== "string" ? IDENTIFIERS.has(key) ? String(parameter) : parameter
        : CAUSES.has(key) && !parameter.startsWith(NATIVE_MESSAGE_PREFIX) ? ownSentence(parameter, language)
          : embeddedMessages(parameter, language)
    }
    // One of Windfall's own words: its translation when the catalog has one.
    if (value.code === "text.term" && typeof params.text === "string") return ownSentence(params.text, language)
    // A message said as a sentence of its own: it ends with a full stop.
    if (value.code === "text.sentence" && typeof params.text === "string")
      return /[.!?…]$/.test(params.text) ? params.text : `${params.text}.`
    const entry = en[value.code as MessageKey]
    const source = typeof entry === "string" ? entry : entry.other
    const required = [...source.matchAll(/\{(\w+)\}/g)].map((match) => match[1])
    if (required.some((key) => !Object.hasOwn(params, key)) ||
      (Object.hasOwn(params, "count") && typeof params.count !== "number"))
      return message("native.messageInvalid", {}, language)
    return message(value.code as MessageKey, params, language)
  } catch { return message("native.messageInvalid", {}, language) }
}
