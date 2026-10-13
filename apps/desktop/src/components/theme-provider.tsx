import { useEffect, type ReactNode } from "react"

import { resolveTheme, useUiStore, type Theme } from "@/lib/store/ui"

const COLOR_SCHEME_QUERY = "(prefers-color-scheme: dark)"

// Without this every hover and focus transition plays at once when the
// theme flips, and the window looks like it is fading.
function withoutTransitions(change: () => void) {
  const style = document.createElement("style")
  // A style rule, not a caption.
  style.append("*,*::before,*::after{transition:none!important;animation:none!important}")
  document.head.appendChild(style)
  change()
  window.getComputedStyle(document.body).getPropertyValue("color")
  requestAnimationFrame(() => requestAnimationFrame(() => style.remove()))
}

function applyTheme(theme: Theme) {
  const resolved = resolveTheme(theme)
  const root = document.documentElement
  if (root.classList.contains(resolved)) return
  withoutTransitions(() => {
    root.classList.remove("light", "dark")
    root.classList.add(resolved)
  })
}

/**
 * Keeps the `dark` or `light` class on the document in step with the theme
 * in the UI store. The theme itself is changed through the store or the
 * `view.toggleTheme` action.
 */
export function ThemeProvider({ children }: { children: ReactNode }) {
  const theme = useUiStore((state) => state.theme)

  useEffect(() => {
    applyTheme(theme)
    if (theme !== "system") return undefined
    const query = window.matchMedia(COLOR_SCHEME_QUERY)
    const onChange = () => applyTheme("system")
    query.addEventListener("change", onChange)
    return () => query.removeEventListener("change", onChange)
  }, [theme])

  return children
}

export function useTheme() {
  const theme = useUiStore((state) => state.theme)
  const setTheme = useUiStore((state) => state.setTheme)
  return { theme, setTheme, resolvedTheme: resolveTheme(theme) }
}
