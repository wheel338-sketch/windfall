import { toast } from "sonner"
import { createElement } from "react"

import { errorSource } from "@/lib/ipc/backend"
import { localizeBackendText, nativeMessage, translatedText } from "@/lib/i18n"
import { LocalizedDiagnostic, LocalizedLabel } from "@/lib/i18n/diagnostic"
import type { DynamicActionMessage } from "@/lib/i18n/action-messages"

/**
 * Shows a failed action to the user. Every rejected backend call ends up
 * here, so nothing fails silently.
 */
export function reportError(error: unknown, what?: string | DynamicActionMessage) {
  const source = errorSource(error)
  if (nativeMessage(source) !== null || typeof what === "object") {
    const diagnostic = createElement(LocalizedDiagnostic, { source })
    if (what) toast.error(createElement(LocalizedLabel, { source: what }), { description: diagnostic })
    else toast.error(diagnostic)
    return
  }
  const message = localizeBackendText(source)
  if (what) toast.error(translatedText(what), { description: message })
  else toast.error(message)
}

/**
 * Tells the user that an edit was not made, and why. For an edit the UI
 * turns down itself, before anything is sent to the backend.
 */
export function refuse(what: string, why: string) {
  reportError(why, what)
}

/** Awaits a backend call. A failure is shown and turned into `null`. */
export async function attempt<T>(
  work: Promise<T>,
  what?: string | DynamicActionMessage
): Promise<T | null> {
  try {
    return await work
  } catch (error) {
    reportError(error, what)
    return null
  }
}
