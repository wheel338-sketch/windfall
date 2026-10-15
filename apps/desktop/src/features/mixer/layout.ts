/** Width of an insert strip in pixels. Every strip is the same width. */
import { translatedText as t } from "@/lib/i18n"
export const STRIP_WIDTH = 80
export const MIXER_LAYOUTS = [
  { value: "adaptive", get label() { return t("Adaptive") }, width: 80 },
  { value: "compact", get label() { return t("Compact") }, width: 36, mode: "mini" },
  { value: "compact2", get label() { return t("Compact 2") }, width: 48, mode: "flat" },
  { value: "narrow", get label() { return t("Narrow") }, width: 64, mode: "tight" },
  { value: "standard", get label() { return t("Standard") }, width: 80, mode: "compact" },
  { value: "wide", get label() { return t("Wide") }, width: 104, mode: "full" },
  { value: "extraWide", get label() { return t("Extra wide") }, width: 128, mode: "full" },
  { value: "large", get label() { return t("Large") }, width: 152, mode: "full" },
  { value: "extraLarge", get label() { return t("Extra large") }, width: 176, mode: "full" },
] as const

/** The widest an adaptive strip grows to, the "Extra wide" layout's width. */
export const ADAPTIVE_MAX_WIDTH = 128

/**
 * The width of the strips in the adaptive layout: `count` strips share
 * `room` pixels, beside the add column, so a few tracks spread out and keep
 * their names readable, and many fall back to the standard width and scroll.
 * Unmeasured (0) room gets the standard width.
 */
export function adaptiveStripWidth(room: number, count: number): number {
  if (room <= 0 || count <= 0) return STRIP_WIDTH
  const share = Math.floor((room - ADD_WIDTH) / count)
  return Math.max(STRIP_WIDTH, Math.min(ADAPTIVE_MAX_WIDTH, share))
}

export function chosenStripLayout(choice: string, height: number, sends: number, effects: number): StripLayout & { width: number } {
  const selected = MIXER_LAYOUTS.find((layout) => layout.value === choice) ?? MIXER_LAYOUTS[0]
  const automatic = stripLayout(height, sends, effects)
  const order: StripMode[] = ["full", "compact", "tight", "flat", "mini"]
  const wanted = "mode" in selected ? selected.mode : automatic.mode
  const mode = order.indexOf(wanted) > order.indexOf(automatic.mode) ? wanted : automatic.mode
  return { ...automatic, mode, sendRows: mode === "full" ? automatic.sendRows : 0, effectRows: mode === "mini" || mode === "flat" ? 0 : automatic.effectRows, width: selected.width }
}
export const MASTER_WIDTH = 92
/** Width of the "add track" column after the last strip. */
export const ADD_WIDTH = 44

/** The least width the effects beside the strips are useful at. */
export const INSPECTOR_MIN_WIDTH = 288
/**
 * The width the effects start at: enough for an editor to put its controls
 * beside its display, which the equaliser and the compressor do from
 * 26rem of their own.
 */
export const INSPECTOR_WIDTH = 460
/** The most of the window's width the effects start at. */
const INSPECTOR_WINDOW_SHARE = 0.36

/**
 * The width the effects beside the strips start at in a window
 * `windowWidth` pixels wide: the full width when there is room, and less
 * in a small window, which keeps most of the mixer for the strips.
 */
export function inspectorWidthFor(windowWidth: number): number {
  const share = Math.round(windowWidth * INSPECTOR_WINDOW_SHARE)
  return Math.max(INSPECTOR_MIN_WIDTH, Math.min(INSPECTOR_WIDTH, share))
}

/** Strips kept mounted on each side of the ones in view. */
const OVERSCAN = 2

/*
 * How much of a strip fits at a given panel height, from tall to low.
 *
 * - "full": everything inline, including the output selector and the sends.
 *   The effect rack is tall enough for the longest chain in the mixer and
 *   a row to add to it, up to `MAX_EFFECT_ROWS`.
 * - "compact": the output and sends move into a popover behind a button in
 *   the header, so the fader keeps a usable length. The rack gets the rows
 *   that fit above the shortest useful fader and scrolls a longer chain.
 * - "tight": the channel chips join the popover and the peak readout goes.
 *   The rack keeps two rows, and the fader gets the rest.
 * - "flat": too low for an upright fader. It lies across the strip, with
 *   the readouts in a line under it. The rack shrinks to a badge, "FX" and
 *   the number of effects, in a row of its own; it opens the chain in the
 *   inspector.
 * - "mini": the flat fader with no readouts. The badge moves into the name
 *   row and shows only on tracks that have effects; the routing button
 *   makes room for it and sits beside mute and solo.
 */
export type StripMode = "full" | "compact" | "tight" | "flat" | "mini"

export type StripLayout = {
  mode: StripMode
  /** Send rows shown inline in "full" mode. Every strip keeps this many. */
  sendRows: number
  /**
   * Rows of the effect rack. Every strip gets this many, so the faders
   * line up. 0 means the rack is a badge.
   */
  effectRows: number
}

/** Most send rows shown inline. A track with more scrolls its list. */
export const MAX_SEND_ROWS = 3
export const SEND_ROW_HEIGHT = 26

export const EFFECT_ROW_HEIGHT = 18
/** Most rack rows shown at once. A longer chain scrolls inside the rack. */
export const MAX_EFFECT_ROWS = 5
/** The fewest rows a rack is shown with: an effect and the row to add one. */
export const MIN_EFFECT_ROWS = 2
/** Space between the rack and what follows it. */
const RACK_GAP = 6
/** The badge's own row in the "flat" layout. */
const BADGE_ROW = 18

/** The least height an upright fader is useful at. */
const MIN_FADER = 70

// What everything but the fader and the effects takes up in each mode, in
// pixels. These follow the rows in `strip.tsx`.
const FULL_ROWS = 183
const COMPACT_ROWS = 126
const TIGHT_ROWS = 84
const FLAT_ROWS = 112

/** Height of a rack of `rows` rows with the space under it. */
function rackHeight(rows: number): number {
  return rows * EFFECT_ROW_HEIGHT + RACK_GAP
}

/**
 * Picks the layout for strips `height` pixels tall when the busiest track
 * has `maxSends` sends and the longest chain has `maxEffects` effects. A
 * height of 0 means "not measured yet" and gets the full layout.
 */
export function stripLayout(
  height: number,
  maxSends: number,
  maxEffects = 0
): StripLayout {
  const sendRows = Math.min(Math.max(0, maxSends), MAX_SEND_ROWS)
  // One row more than the longest chain, for adding to it.
  const wanted = Math.min(
    Math.max(MIN_EFFECT_ROWS, maxEffects + 1),
    MAX_EFFECT_ROWS
  )
  if (height <= 0) return { mode: "full", sendRows, effectRows: wanted }

  const fullRows = FULL_ROWS + sendRows * SEND_ROW_HEIGHT + rackHeight(wanted)
  if (height >= fullRows + 110) {
    return { mode: "full", sendRows, effectRows: wanted }
  }
  // Below the full layout the fader keeps its shortest useful length and
  // the rack takes what is left, up to the rows it wants.
  const fits = (rows: number) =>
    Math.min(
      wanted,
      Math.floor((height - rows - MIN_FADER - RACK_GAP) / EFFECT_ROW_HEIGHT)
    )
  const compact = fits(COMPACT_ROWS)
  if (compact >= MIN_EFFECT_ROWS) {
    return { mode: "compact", sendRows: 0, effectRows: compact }
  }
  if (fits(TIGHT_ROWS) >= MIN_EFFECT_ROWS) {
    return { mode: "tight", sendRows: 0, effectRows: MIN_EFFECT_ROWS }
  }
  return {
    mode: height >= FLAT_ROWS + BADGE_ROW ? "flat" : "mini",
    sendRows: 0,
    effectRows: 0,
  }
}

export type StripRange = {
  /** Index of the first strip to mount. */
  start: number
  /** One past the last strip to mount. */
  end: number
}

/**
 * The strips to mount: the ones the scrolled view shows plus a couple on
 * each side, so a strip is ready before it scrolls in.
 */
export function visibleRange(
  scrollLeft: number,
  viewWidth: number,
  count: number,
  width = STRIP_WIDTH
): StripRange {
  const first = Math.floor(Math.max(0, scrollLeft) / width)
  const last = Math.ceil((Math.max(0, scrollLeft) + viewWidth) / width)
  return {
    start: Math.max(0, Math.min(count, first - OVERSCAN)),
    end: Math.max(0, Math.min(count, last + OVERSCAN)),
  }
}

/**
 * Where to scroll so strip `index` is fully in view, or null when it
 * already is.
 */
export function scrollToReveal(
  index: number,
  scrollLeft: number,
  viewWidth: number,
  width = STRIP_WIDTH
): number | null {
  const left = index * width
  const right = left + width
  if (left < scrollLeft) return left
  if (right > scrollLeft + viewWidth) return Math.max(0, right - viewWidth)
  return null
}
