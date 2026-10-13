import { Fragment, type ReactNode } from "react"

import { useLanguageStore } from "."

/**
 * Mounts what it wraps again when the interface language changes. Most text
 * is translated while a component renders, but some is kept in memoized
 * values, drawn on a canvas or set by an effect, and a new mount is what
 * produces all of it again. The project, the engine and what is open live in
 * stores, so they are unchanged; a half-typed field inside is not kept.
 */
export function LanguageBoundary({ children }: { children: ReactNode }) {
  const language = useLanguageStore((state) => state.language)
  return <Fragment key={language}>{children}</Fragment>
}
