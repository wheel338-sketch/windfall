import type { ChannelId, Command, Note, NoteCurvePoint, NoteExpressionCurve, NoteId, PatternId } from "@/bindings"
import { curveShape } from "@/lib/automation/curve"
import { onProjectReplaced } from "@/lib/store/replaced"
import { translatedText as t } from "@/lib/i18n"

// The current note model has no mute bit. Silence is a real velocity edit;
// retain the previous value while this project is open for reversible toggles.
const velocities = new Map<string, number>()
onProjectReplaced(() => velocities.clear())

export function rememberedVelocity(pattern: PatternId, channel: ChannelId, noteId: NoteId): number | undefined {
  return velocities.get(`${pattern}:${channel}:${noteId}`)
}

export function toggledVelocity(pattern: PatternId, channel: ChannelId, note: Note, fallback: number): number {
  const key = `${pattern}:${channel}:${note.id}`
  if (note.velocity > 0) {
    velocities.set(key, note.velocity)
    return 0
  }
  return velocities.get(key) ?? (fallback > 0 ? fallback : 0.8)
}

/** Restrict a normalized expression curve, including its held/bent segments. */
function cropCurve(points: readonly NoteCurvePoint[], start: number, end: number): NoteCurvePoint[] {
  if (!points.length) return []
  function boundary(position: number): NoteCurvePoint {
    const next = points.findIndex((point) => point.position > position)
    const from = next < 0 ? points.at(-1)! : points[Math.max(0, next - 1)]
    const to = next < 0 ? undefined : points[next]
    const value = !to || from.hold || position <= from.position ? from.value
      : from.value + (to.value - from.value) * curveShape((position - from.position) / (to.position - from.position), from.curve)
    return { ...from, position, value }
  }
  const cropped = [boundary(start), ...points.filter((point) => point.position > start && point.position < end).map((point) => ({ ...point })), boundary(end)]
  return cropped.map((point, index) => {
    const next = cropped[index + 1]
    const sourceEnd = points.find((source) => source.position > point.position)
    const sourceStart = points.findLast((source) => source.position <= point.position)
    const curve = next && sourceStart && sourceEnd
      ? sourceStart.curve * (next.position - point.position) / (sourceEnd.position - sourceStart.position)
      : 0
    return { ...point, position: (point.position - start) / (end - start), curve }
  })
}

/** Keep the original id on the left and insert the right half in one history step. */
export function sliceNoteCommand(
  target: { pattern: PatternId; channel: ChannelId },
  note: Note,
  tick: number,
  curves: readonly NoteExpressionCurve[] = []
): Command | null {
  const split = Math.round(tick)
  const length = split - note.start
  if (!Number.isFinite(split) || length <= 0 || length >= note.length) return null
  const sourceCurves = curves.filter((curve) => curve.note === note.id)
  const fraction = length / note.length
  const commands: Command[] = []
  if (sourceCurves.length) commands.push({
    type: "setNoteExpressionCurves", ...target, expected: [note], expectedCurves: sourceCurves,
    curves: sourceCurves.map((curve) => ({ ...curve, points: cropCurve(curve.points, 0, fraction) })),
  })
  commands.push({ type: "updateNotes", ...target, updates: [{ id: note.id, patch: { length } }] })
  const right = { start: split, length: note.length - length, key: note.key, velocity: note.velocity, pan: note.pan, expression: note.expression ? { ...note.expression } : undefined }
  commands.push(sourceCurves.length ? {
    type: "addNotesWithCurves", ...target, notes: [right],
    curves: sourceCurves.map((curve) => ({ noteIndex: 0, parameter: curve.parameter, points: cropCurve(curve.points, fraction, 1) })),
  } : { type: "addNotes", ...target, notes: [right] })
  return { type: "batch", label: t("Slice note"), commands }
}
