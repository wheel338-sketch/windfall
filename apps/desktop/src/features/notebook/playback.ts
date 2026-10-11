import type { NotebookPage } from "@/bindings"

/** Latest cue wins; equal ticks select the earlier page. Seek/loop is stateless. */
export function pageAtTick(pages: readonly NotebookPage[], tick: number): number | null {
  if (!Number.isFinite(tick) || tick < 0) return null
  let chosen: number | null = null
  let latest = -1
  pages.forEach((page, index) => {
    const cue = page.startTick
    if (cue !== undefined && cue !== null && cue <= tick && cue > latest) {
      latest = cue
      chosen = index
    }
  })
  return chosen
}
