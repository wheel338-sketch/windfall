import type { HitPart } from "@/lib/canvas"

import type { InnerHit } from "./inner"

export type Tool =
  "draw" | "paint" | "select" | "erase" | "mute" | "slip" | "playback" | "slice"

export const TOOLS: readonly Tool[] = [
  "draw",
  "paint",
  "select",
  "erase",
  "mute",
  "slip",
  "playback",
  "slice",
]

/** What was under the pointer when a button went down. */
export type Press = {
  tool: Tool
  /** 0 left, 1 middle, 2 right, as on a pointer event. */
  button: number
  /** Ctrl, or Cmd on macOS. */
  mod: boolean
  shift: boolean
  /** The clip under the pointer and which part of it, or null on empty grid. */
  hit: { id: number; part: HitPart } | null
  /**
   * The part of that clip with an edit of its own, if the pointer is on
   * one: a handle of an audio clip, the curve of an automation clip.
   */
  inner?: InnerHit | null
  /**
   * The right button opens the menu in every tool but Playback, as it does
   * in Select, instead of deleting. Off by default: FL Studio's way.
   */
  rightClickMenu?: boolean
}

/** What a press starts. The pointer handling carries it out. */
export type Intent =
  | { kind: "none" }
  | { kind: "pan" }
  /** Seek the transport at the pointer throughout the gesture. */
  | { kind: "playback" }
  /** Split every clip crossing a vertical line when the button is released. */
  | { kind: "slice" }
  /** Drag a box around clips. `additive` keeps what was already selected. */
  | { kind: "marquee"; additive: boolean }
  | { kind: "move"; id: number; additive: boolean }
  | { kind: "trim-start"; id: number }
  | { kind: "resize-end"; id: number }
  /** Slide a clip's content inside its fixed window. */
  | { kind: "slip"; id: number }
  /** Put one clip of the brush where the button is released. */
  | { kind: "place" }
  /** Lay clips of the brush back to back along the stroke. */
  | { kind: "paint" }
  /** Delete every clip the stroke crosses. */
  | { kind: "erase" }
  /** Mute or unmute every clip the stroke crosses. */
  | { kind: "mute" }
  /** Open the right-click menu for the selected clips or the empty grid. */
  | { kind: "menu"; id: number | null }
  /** Drag the end of an audio clip's fade. */
  | { kind: "fade"; id: number; edge: "in" | "out" }
  /** Drag an audio clip's gain up or down. */
  | { kind: "gain"; id: number }
  /** Drag a point of an automation clip's curve. */
  | { kind: "point"; id: number; index: number }
  /** Bend the stretch of a curve that leaves the point at `index`. */
  | { kind: "bend"; id: number; index: number }
  /** Add a point to a curve under the pointer, and drag it. */
  | { kind: "add-point"; id: number }
  | { kind: "delete-point"; id: number; index: number }
  /** Open the right-click menu of a point of a curve. */
  | { kind: "point-menu"; id: number; index: number }

function onClip(hit: { id: number; part: HitPart }, additive: boolean): Intent {
  switch (hit.part) {
    case "start-edge":
      return { kind: "trim-start", id: hit.id }
    case "end-edge":
      return { kind: "resize-end", id: hit.id }
    case "body":
      return { kind: "move", id: hit.id, additive }
    default: {
      const _exhaustive: never = hit.part
      return _exhaustive
    }
  }
}

/**
 * What a left press on a part of a clip with an edit of its own starts, or
 * null when the press is the clip's. A point of a curve is taken even on
 * the clip's edge, where a curve's first point usually is; the rest of the
 * curve leaves the edges to resizing.
 */
function onInner(
  hit: { id: number; part: HitPart },
  inner: InnerHit
): Intent | null {
  const id = hit.id
  switch (inner.kind) {
    case "fade":
      return { kind: "fade", id, edge: inner.edge }
    case "gain":
      return { kind: "gain", id }
    case "point":
      return { kind: "point", id, index: inner.index }
    case "bend":
      return hit.part === "body"
        ? { kind: "bend", id, index: inner.index }
        : null
    case "curve":
      return hit.part === "body" ? { kind: "add-point", id } : null
    default: {
      const _exhaustive: never = inner
      return _exhaustive
    }
  }
}

/**
 * Decides what a press does from the tool, the button, the modifier keys
 * and what is under the pointer. The modifiers follow `lib/edit-modifiers`.
 *
 * - Middle button pans.
 * - Ctrl+drag on empty grid selects with a box except in Playback. On a clip,
 *   Ctrl leaves the press a move in Draw, Paint and Select, which the drop
 *   turns into a copy. Slip keeps the press a content slide.
 * - Shift adds the pressed clip to the selection, or the box's clips.
 * - Right button deletes, except Select opens a menu and Playback ignores it.
 *   With `rightClickMenu` every tool but Playback opens the menu.
 *   On a point of a curve it deletes the point, or opens the point's menu;
 *   on the rest of a curve it does nothing, so a near miss of a point does
 *   not delete the clip.
 * - Draw, Paint and Select all move and resize the clips they press on,
 *   and edit what is inside them: the handles of an audio clip and the
 *   curve of an automation clip, which is moved by its title bar.
 */
export function intentFor(press: Press): Intent {
  const { tool, button, mod, shift, hit } = press
  const editsClips = tool === "draw" || tool === "paint" || tool === "select"
  const inner = hit && editsClips ? (press.inner ?? null) : null
  if (button === 1) return { kind: "pan" }
  if (button === 2) {
    if (tool === "playback") return { kind: "none" }
    const menu = tool === "select" || press.rightClickMenu === true
    if (hit && inner?.kind === "point") {
      return menu
        ? { kind: "point-menu", id: hit.id, index: inner.index }
        : { kind: "delete-point", id: hit.id, index: inner.index }
    }
    if (menu) return { kind: "menu", id: hit?.id ?? null }
    return inner?.kind === "bend" || inner?.kind === "curve"
      ? { kind: "none" }
      : { kind: "erase" }
  }
  if (button !== 0) return { kind: "none" }
  // The Erase and Mute tools do nothing else to a clip, so there Ctrl
  // selects on a clip as well.
  if (mod && tool !== "playback" && !(hit && (editsClips || tool === "slip")))
    return { kind: "marquee", additive: shift }
  // With Ctrl a press anywhere on a clip is the clip's: a move, or a copy.
  if (hit && inner && !mod) {
    const intent = onInner(hit, inner)
    if (intent) return intent
  }

  switch (tool) {
    case "draw":
      return hit ? onClip(hit, shift) : { kind: "place" }
    case "paint":
      return hit ? onClip(hit, shift) : { kind: "paint" }
    case "select":
      return hit ? onClip(hit, shift) : { kind: "marquee", additive: shift }
    case "erase":
      return { kind: "erase" }
    case "mute":
      return { kind: "mute" }
    case "playback":
      return { kind: "playback" }
    case "slice":
      return { kind: "slice" }
    case "slip":
      return hit ? { kind: "slip", id: hit.id } : { kind: "none" }
    default: {
      const _exhaustive: never = tool
      return _exhaustive
    }
  }
}

/** The mouse cursor for a tool over a part of a clip, or over empty grid. */
export function cursorFor(
  tool: Tool,
  part: HitPart | null,
  inner: InnerHit | null = null
): string {
  if (tool === "erase") return part ? "not-allowed" : "default"
  if (tool === "mute") return part ? "pointer" : "default"
  if (tool === "slip") return part ? "ew-resize" : "default"
  if (tool === "playback") return "crosshair"
  if (tool === "slice") return "crosshair"
  const edge = part === "start-edge" || part === "end-edge"
  if (inner) {
    if (inner.kind === "fade") return "ew-resize"
    if (inner.kind === "gain") return "ns-resize"
    if (inner.kind === "point") return "move"
    if (!edge) return inner.kind === "bend" ? "ns-resize" : "crosshair"
  }
  if (edge) return "ew-resize"
  if (part === "body") return "grab"
  return tool === "select" ? "default" : "crosshair"
}
