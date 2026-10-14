import type { Backend } from "./backend"
import { createTauriBackend } from "./tauri"

export { errorMessage, errorSource } from "./backend"
export type { Backend, StoredAudioSettings, Unsubscribe } from "./backend"

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window
}

// The mock brings the document as a WebAssembly module, which the app never
// needs. It is fetched as a chunk of its own, and only outside the app. The
// module waits here until that chunk is ready, so everything that imports
// `backend` can go on using it at once.
const mock = isTauri() ? null : await import("./mock")

let current: Backend | null = null

function resolve(): Backend {
  current ??= mock ? mock.createMockBackend() : createTauriBackend()
  return current
}

/** Swaps the backend. Tests use this to start from a fresh mock. */
export function setBackend(next: Backend) {
  current = next
}

/**
 * The one backend the UI talks to: Tauri inside the app, the mock in a plain
 * browser. It is created on first use, and every call is forwarded to
 * whichever backend is current.
 */
export const backend: Backend = new Proxy({} as Backend, {
  get: (_target, key) => Reflect.get(resolve(), key),
})
