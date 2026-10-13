import { createContext, useContext } from "react"

import type { PianoRollSession } from "./session"
import { translatedText as t } from "@/lib/i18n"

export const SessionContext = createContext<PianoRollSession | null>(null)

/** The open piano roll's session. Only for components inside the panel. */
export function useSession(): PianoRollSession {
  const session = useContext(SessionContext)
  if (!session) throw new Error(t("useSession needs a piano roll around it"))
  return session
}
