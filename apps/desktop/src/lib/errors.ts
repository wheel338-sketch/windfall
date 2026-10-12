import { toast } from "sonner"

import { errorMessage } from "@/lib/ipc/backend"
import { translatedText } from "@/lib/i18n"

/**
 * Shows a failed action to the user. Every rejected backend call ends up
 * here, so nothing fails silently.
 */
export function reportError(error: unknown, what?: string) {
  const message = translatedText(errorMessage(error))
  if (what) toast.error(translatedText(what), { description: message })
  else toast.error(message)
}

/**
 * Tells the user that an edit was not made, and why. For an edit the UI
 * turns down itself, before anything is sent to the backend.
 */
export function refuse(what: string, why: string) {
  toast.error(translatedText(what), { description: translatedText(why) })
}

/** Awaits a backend call. A failure is shown and turned into `null`. */
export async function attempt<T>(
  work: Promise<T>,
  what?: string
): Promise<T | null> {
  try {
    return await work
  } catch (error) {
    reportError(error, what)
    return null
  }
}
