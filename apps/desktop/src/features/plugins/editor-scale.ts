import { backend } from "@/lib/ipc"
import { reportError } from "@/lib/errors"
import { UI_SCALE_EVENT, uiScaleFactor } from "@/lib/ui-scale"

/** One per webview. Serializes/coalesces changes so an earlier slow native call
 * cannot finish after the latest preference. Native runtime remembers reopen. */
export function connectNativeEditorScale(
  sync = (application: number) => backend.kind === "tauri"
    ? backend.pluginEditorScale(application)
    : Promise.resolve(),
  fail = (error: unknown) => reportError(error, "Could not scale native plugin content")
) {
  let alive = true
  let running = false
  let desired = uiScaleFactor()
  let applied: number | undefined
  async function flush() {
    if (running || !alive) return
    running = true
    try {
      while (alive && applied !== desired) {
        const next = desired
        // A refusal is reported once per selection; another selection can retry.
        applied = next
        try { await sync(next) } catch (error) { if (alive) fail(error) }
      }
    } finally { running = false }
  }
  const change = () => { desired = uiScaleFactor(); void flush() }
  window.addEventListener(UI_SCALE_EVENT, change)
  void flush()
  return () => { alive = false; window.removeEventListener(UI_SCALE_EVENT, change) }
}
