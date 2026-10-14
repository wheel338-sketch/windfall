import type { HitPart } from "@/lib/canvas"
import { MODIFIER_HINTS } from "@/lib/edit-modifiers"

import type { Tool } from "./store"

/*
 * What a press will do, decided from the tool, the button, the modifiers
 * and what is under the pointer. The cursor and the status-bar hint come
 * from the same answer, so they never promise something else. The
 * modifiers follow `lib/edit-modifiers`, as the playlist's do.
 */

export type Intent =
  | { kind: "draw" }
  | { kind: "paint"; drum?: boolean }
  | { kind: "erase" }
  | { kind: "mute" }
  | { kind: "slice" }
  | { kind: "zoom" }
  | { kind: "playback" }
  | { kind: "marquee" }
  | { kind: "move" }
  | { kind: "resize"; edge: "start" | "end" }
  /** Right-click in the select tool opens the menu. */
  | { kind: "menu" }

export type PressButton = "left" | "right"

export type PressModifiers = { ctrl: boolean }

export function pressIntent(
  tool: Tool,
  button: PressButton,
  hitPart: HitPart | null,
  modifiers: PressModifiers,
  drum = false
): Intent {
  if (button === "right") {
    return tool === "select" ? { kind: "menu" } : { kind: "erase" }
  }
  if (tool === "paint" && drum) return { kind: "paint", drum: true }
  switch (tool) {
    case "mute": return { kind: "mute" }
    case "slice": return { kind: "slice" }
    case "zoom": return { kind: "zoom" }
    case "playback": return { kind: "playback" }
    case "draw":
    case "paint":
    case "select":
    case "erase": break
    default: {
      const _exhaustive: never = tool
      return _exhaustive
    }
  }
  // Ctrl selects with a box, except on a note the tool can move: there it
  // stays a move, which the drop turns into a copy.
  if (modifiers.ctrl && (hitPart === null || tool === "erase")) {
    return { kind: "marquee" }
  }
  if (tool === "erase") return { kind: "erase" }
  if (hitPart === "body") return { kind: "move" }
  if (hitPart === "start-edge") return { kind: "resize", edge: "start" }
  if (hitPart === "end-edge") return { kind: "resize", edge: "end" }
  if (tool === "draw") return { kind: "draw" }
  if (tool === "paint") return { kind: "paint" }
  return { kind: "marquee" }
}

function svgCursor(body: string, hotX: number, hotY: number, fallback: string) {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="none" stroke-linejoin="round" stroke-linecap="round">${body}</svg>`
  return `url("data:image/svg+xml,${encodeURIComponent(svg)}") ${hotX} ${hotY}, ${fallback}`
}

// A white shape with a dark outline reads on both themes and on any note.
const OUTLINE = `stroke="#111" stroke-width="1.25" fill="#fff"`

const PENCIL_CURSOR = svgCursor(
  `<path d="M2.5 17.5l1.1-4.2L13.4 3.5a1.6 1.6 0 0 1 2.3 0l.8.8a1.6 1.6 0 0 1 0 2.3L6.7 16.4z" ${OUTLINE}/><path d="M12 5l3 3" stroke="#111" stroke-width="1.25"/>`,
  2,
  18,
  "crosshair"
)

const BRUSH_CURSOR = svgCursor(
  `<path d="M9.2 10.2l6-7a1.3 1.3 0 0 1 1.9 1.7l-6.6 6.5z" ${OUTLINE}/><path d="M2.5 17.5c2.2.3 4.6 0 5.8-1.2a2.6 2.6 0 0 0-3.6-3.7c-1.2 1.2-.6 3.3-2.2 4.9z" ${OUTLINE}/>`,
  2,
  18,
  "crosshair"
)

const ERASER_CURSOR = svgCursor(
  `<path d="M3 13.2l7.8-7.8a1.4 1.4 0 0 1 2 0l3.3 3.3a1.4 1.4 0 0 1 0 2l-5.7 5.8H6.3z" ${OUTLINE}/><path d="M7.2 9l4.700 4.700M6 16.5h11" stroke="#111" stroke-width="1.25"/>`,
  5,
  16,
  "not-allowed"
)

export function cursorFor(intent: Intent | null): string {
  if (!intent) return "default"
  switch (intent.kind) {
    case "draw":
      return PENCIL_CURSOR
    case "paint":
      return BRUSH_CURSOR
    case "erase":
      return ERASER_CURSOR
    case "mute": return "pointer"
    case "slice": return "col-resize"
    case "zoom": return "zoom-in"
    case "playback": return "ew-resize"
    case "marquee":
      return "crosshair"
    case "move":
      return "move"
    case "resize":
      return "ew-resize"
    case "menu":
      return "default"
    default: {
      const _exhaustive: never = intent
      return _exhaustive
    }
  }
}

/** What the left button will do, for the status bar. */
export function hintFor(intent: Intent | null, tool: Tool): string | null {
  if (!intent) return null
  const rightClick =
    tool === "select"
      ? "Right-click for the menu"
      : "Right-click or right-drag deletes"
  const { copy, add, free } = MODIFIER_HINTS
  switch (intent.kind) {
    case "draw":
      return `Click to add a note, drag to place it. ${free}. Ctrl+drag selects. ${rightClick}`
    case "paint":
      if (intent.drum) return `Drag to toggle drum steps. The first cell chooses add or delete. Escape cancels. ${rightClick}`
      return `Drag to paint a row of notes. ${free}. Ctrl+drag selects. ${rightClick}`
    case "erase":
      return "Click or drag across notes to delete them"
    case "mute": return "Click a note to mute or unmute it"
    case "slice": return `Click inside a note to split it. ${free}`
    case "zoom": return "Drag a rectangle to zoom. Click again or press Escape to restore the view"
    case "playback": return "Hold and drag to seek and audition notes. Release or press Escape to stop auditioning"
    case "marquee":
      return "Drag to select notes. Shift adds to the selection. Click empty space to clear it"
    case "move":
      return `Drag to move. ${copy}. ${add}. ${free}. ${rightClick}`
    case "resize":
      return intent.edge === "end"
        ? `Drag to change the length. ${free}`
        : `Drag to move the start and keep the end. ${free}`
    case "menu":
      return null
    default: {
      const _exhaustive: never = intent
      return _exhaustive
    }
  }
}
