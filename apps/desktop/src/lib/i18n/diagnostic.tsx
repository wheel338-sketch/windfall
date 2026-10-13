import { localizeBackendText, message, useLanguageStore, useTranslation } from "."
import type { DynamicActionMessage } from "./action-messages"

/** Keep the wire message in an already visible toast, rather than its translation. */
export function LocalizedDiagnostic({ source }: { source: string }) {
  useLanguageStore((state) => state.language)
  return <>{localizeBackendText(source)}</>
}

export function LocalizedLabel({ source }: { source: string | DynamicActionMessage }) {
  const t = useTranslation()
  return <>{typeof source === "string" ? t(source) : message(source.key, source.values)}</>
}
