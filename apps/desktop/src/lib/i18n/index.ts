import { create } from "zustand"
import { en } from "./en"
import { es } from "./es"
import { nativeEn } from "./native-messages"
import { actionMessages, dynamicActionMessage } from "./action-messages"
import type { Catalog, Language, MessageKey } from "./types"
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
export function setLanguage(language: Language) {
  useLanguageStore.setState({ language })
  document.documentElement.lang = language
  try { localStorage.setItem(LANGUAGE_STORAGE_KEY, language) } catch { /* Session choice remains usable. */ }
}
/** One listener per window, released with its shell. Storage rehydration never echoes a write. */
export function connectLanguage(): () => void {
  document.documentElement.lang = useLanguageStore.getState().language
  const receive = (event: StorageEvent) => {
    if (event.key !== LANGUAGE_STORAGE_KEY && event.key !== null) return
    const language = load()
    useLanguageStore.setState({ language })
    document.documentElement.lang = language
  }
  window.addEventListener("storage", receive)
  return () => window.removeEventListener("storage", receive)
}
export function formatNumber(value: number, options?: Intl.NumberFormatOptions, language = useLanguageStore.getState().language): string {
  return new Intl.NumberFormat(locales[language], options).format(value)
}
export function formatDate(value: Date | number, options?: Intl.DateTimeFormatOptions, language = useLanguageStore.getState().language): string {
  return new Intl.DateTimeFormat(locales[language], options).format(value)
}
export function message(key: MessageKey, values: Record<string, string | number> = {}, language = useLanguageStore.getState().language): string {
  const entry = catalogs[language][key]
  const count = typeof values.count === "number" && Number.isFinite(values.count) ? values.count : 0
  const template = typeof entry === "string" ? entry : entry[new Intl.PluralRules(locales[language]).select(count) === "one" ? "one" : "other"]
  return template.replace(/\{(\w+)\}/g, (placeholder, name: string) => {
    const value = values[name]
    return value === undefined ? placeholder : typeof value === "number" ? formatNumber(value, undefined, language) : value
  })
}
/** Transitional display adapter for registry/toolbar labels. Never use on project/user text. */
export function translatedText(value: string, language = useLanguageStore.getState().language): string {
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
export function actionTitle(id: string, fallback: string, language = useLanguageStore.getState().language): string {
  const dynamic = dynamicActionMessage(id)
  if (dynamic) return message(dynamic.key, dynamic.values, language)
  const key = actionMessages[id]
  return key ? message(key, {}, language) : id.startsWith("file.openRecent.") ? fallback : translatedText(fallback, language)
}

/** English compatibility title plus bounded registry-owned translation metadata. */
export function dynamicTitle(key: MessageKey, values: Record<string, string | number>) {
  return { title: message(key, values, "en"), titleMessage: { key, values } }
}

export function localizeBackendText(text: string): string {
  const native = nativeMessage(text)
  if (native !== null) return native
  if (Object.hasOwn(en, text)) return translatedText(text)
  const prefixes = [
    ["Installation failed. Keep the current installation and retry. ", "Installation failed. Keep the current installation and retry. {detail}"],
    ["Download or signature verification failed. Nothing was installed. ", "Download or signature verification failed. Nothing was installed. {detail}"],
    ["Update check failed; retry when online. ", "Update check failed; retry when online. {detail}"],
  ] as const
  for (const [prefix,key] of prefixes) if (text.startsWith(prefix)) return message(key,{detail:text.slice(prefix.length)})
  return useLanguageStore.getState().language === "en" ? text : message("Native diagnostic: {detail}", { detail: text })
}

const NATIVE_MESSAGE_PREFIX = "windfall-message:"

/** Decode only the reserved native envelope, leaving authored text untouched. */
export function nativeMessage(text: string, language = useLanguageStore.getState().language): string | null {
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
      params[key] = parameter
    }
    const entry = en[value.code as MessageKey]
    const source = typeof entry === "string" ? entry : entry.other
    const required = [...source.matchAll(/\{(\w+)\}/g)].map((match) => match[1])
    if (required.some((key) => !Object.hasOwn(params, key)) ||
      (Object.hasOwn(params, "count") && typeof params.count !== "number"))
      return message("native.messageInvalid", {}, language)
    return message(value.code as MessageKey, params, language)
  } catch { return message("native.messageInvalid", {}, language) }
}
