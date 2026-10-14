/*
 * What the modifier keys mean while the pointer edits notes in the piano
 * roll or clips on the playlist. Both editors follow this one scheme, and a
 * key never means one thing when the button goes down and another when it
 * comes up:
 *
 * - Ctrl (Cmd on macOS) on a note or clip: the drag copies. It is read when
 *   the button comes up, so it can be pressed or let go mid-drag.
 * - Ctrl on empty grid: the drag selects with a box, whatever the tool.
 * - Shift adds to the selection: a click on a note or clip adds it or takes
 *   it out, and a box adds what it covers. It never copies and never
 *   touches the snap.
 * - Alt lets go of the snap for one drag.
 *
 * FL equivalent: Shift+drag clones and Alt bypasses snap. The mouse works
 * the same in both keymap presets; a preset only changes keys.
 */

/** The same words in both editors' status-bar hints. */
import { translatedText as t } from "@/lib/i18n"
export const MODIFIER_HINTS = {
  copy: "Ctrl+drag copies",
  select: "Ctrl+drag on empty space selects",
  add: "Shift+click adds",
  get free() { return t("Alt: no snap") },
} as const

/** Whether a drag ignores the snap: only while Alt is held. */
export function ignoresSnap(input: { alt: boolean }): boolean {
  return input.alt
}
