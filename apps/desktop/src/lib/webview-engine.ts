/*
 * Which browser engine draws the window: Chromium (WebView2) on Windows,
 * WKWebView on macOS, WebKitGTK on Linux. The stylesheet reads it from
 * `<html data-webview>` for the few places an engine needs its own rule.
 */

export type WebviewEngine = "chromium" | "wkwebview" | "webkitgtk" | "other"

export function webviewEngine(userAgent: string): WebviewEngine {
  if (/\b(Edg|Chrome)\//.test(userAgent)) return "chromium"
  if (!/\bAppleWebKit\//.test(userAgent) || /Android/.test(userAgent)) return "other"
  if (/\b(X11|Linux)\b/.test(userAgent)) return "webkitgtk"
  if (/\bMac OS X\b/.test(userAgent)) return "wkwebview"
  return "other"
}

/** Marks the document before the first render: scrollers take their
 * scrollbar style when they are laid out, so it must be in force by then. */
export function markWebviewEngine(root: HTMLElement = document.documentElement): WebviewEngine {
  const engine = webviewEngine(navigator.userAgent)
  root.dataset.webview = engine
  return engine
}
