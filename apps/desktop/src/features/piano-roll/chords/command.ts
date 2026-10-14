import type { ChannelId, Command } from "@/bindings"
import { DEFAULT_VELOCITY, MAX_PATTERN_TICKS } from "@/lib/units"

import {
  noteInsertionCommand,
  withExtension,
  type PatternInfo,
} from "../edit-math"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

/** Three simultaneous notes, including any pattern extension, in one dispatch. */
export function chordInsertCommand(
  target: { pattern: PatternInfo; channel: ChannelId },
  keys: readonly number[],
  start: number,
  length: number
): Command {
  if (
    keys.length !== 3 ||
    new Set(keys).size !== 3 ||
    keys.some((key) => !Number.isInteger(key) || key < 0 || key > 127)
  ) {
    throw new Error(t("A triad needs three distinct MIDI keys from 0 to 127."))
  }
  if (
    !Number.isInteger(start) ||
    start < 0 ||
    !Number.isInteger(length) ||
    length < 1 ||
    start + length > MAX_PATTERN_TICKS
  ) {
    throw new Error(
      message("Use whole ticks, a positive length, and an end at or before {maxPatternTicks}.", { maxPatternTicks: plainNumber(MAX_PATTERN_TICKS) })
    )
  }
  const notes = keys.map((key) => ({
    key,
    start,
    length,
    velocity: DEFAULT_VELOCITY,
    pan: 0,
  }))
  return withExtension(
    noteInsertionCommand(
      { pattern: target.pattern.id, channel: target.channel },
      notes
    ),
    "Insert chord",
    target.pattern,
    start + length
  )
}
