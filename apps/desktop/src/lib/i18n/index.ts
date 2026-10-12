import { create } from "zustand"
import { en } from "./en"
import { es } from "./es"
import { actionMessages } from "./action-messages"
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
  const key = actionMessages[id]
  return key ? message(key, {}, language) : id.startsWith("file.openRecent.") ? fallback : translatedText(fallback, language)
}
