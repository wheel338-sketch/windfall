import { message } from "@/lib/i18n"
import type { TimelineMarker } from "@/bindings"

/** Song seek destinations, including the start of loop and skip regions. */
export function markerJumps(markers: readonly TimelineMarker[]) {
  return [...markers]
    .sort((a, b) => a.tick - b.tick || a.id - b.id)
    .map((marker) => ({
      title: marker.name.trim() ? message("Jump to {name}", { name: marker.name }) : message("Jump to marker"),
      tick: marker.tick,
    }))
}
