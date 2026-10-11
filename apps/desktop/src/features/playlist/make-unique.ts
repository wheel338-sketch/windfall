import type { Clip, ClipId } from "@/bindings"

/**
 * The clips Make unique acts on, in selection order: every kind. A pattern
 * clip gets a copy of its pattern, an audio clip its own source entry, and
 * an automation clip a copy of its curve that only it shows.
 */
export function makeUniqueClipIds(selected: readonly Clip[]): ClipId[] {
  return selected.map((clip) => clip.id)
}
