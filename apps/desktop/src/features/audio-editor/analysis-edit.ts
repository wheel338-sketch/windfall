import type { AudioEditRecipe, AudioNoteOverride, AudioWarpMarker } from "./analysis-types"
import { translatedText as t } from "@/lib/i18n"

// Match AudioEditRecipe::render_markers: edited edges replace transient anchors.
export function renderMarkers(recipe: AudioEditRecipe): AudioWarpMarker[] {
  const anchors = new Map(recipe.markers.map((m) => [m.sourceFrame, m.targetFrame]))
  for (const edit of recipe.overrides) {
    const note = recipe.analysis.notes[edit.note]
    if (!note) continue
    anchors.set(note.startFrame, edit.startFrame)
    anchors.set(note.endFrame, edit.endFrame)
  }
  return [...anchors].map(([sourceFrame, targetFrame]) => ({ sourceFrame, targetFrame }))
    .sort((a, b) => a.sourceFrame - b.sourceFrame)
}

export function mappedFrame(recipe: AudioEditRecipe, source: number): number {
  const markers = renderMarkers(recipe)
  const index = markers.findIndex((m) => m.sourceFrame >= source)
  if (index <= 0) return markers[0].targetFrame
  const a = markers[index - 1], b = markers[index]
  return Math.round(a.targetFrame + (source - a.sourceFrame) *
    (b.targetFrame - a.targetFrame) / (b.sourceFrame - a.sourceFrame))
}
export function noteEdit(recipe: AudioEditRecipe, index: number): AudioNoteOverride {
  const note = recipe.analysis.notes[index]
  return recipe.overrides.find((o) => o.note === index) ?? {
    note: index, midiPitch: note.midiPitch,
    startFrame: mappedFrame(recipe, note.startFrame),
    endFrame: mappedFrame(recipe, note.endFrame),
  }
}
export function overrideNote(recipe: AudioEditRecipe, edit: AudioNoteOverride): AudioEditRecipe {
  return { ...recipe, overrides: [...recipe.overrides.filter((o) => o.note !== edit.note), edit] }
}
export function recipeProblem(recipe: AudioEditRecipe): string | null {
  const markers = recipe.markers
  if (markers.length < 2 || markers.length > 512 ||
    markers[0].sourceFrame !== 0 || markers[0].targetFrame !== 0 ||
    markers.at(-1)?.sourceFrame !== recipe.analysis.frames)
    return t("Keep source endpoints and the origin fixed.")
  const output = markers.at(-1)!.targetFrame
  if (!Number.isSafeInteger(output) || output <= 0 ||
    output / recipe.analysis.sampleRate > 120)
    return t("Output duration must fit within two minutes.")
  for (let i = 1; i < markers.length; ++i) {
    const a = markers[i - 1], b = markers[i]
    const ratio = (b.targetFrame - a.targetFrame) / (b.sourceFrame - a.sourceFrame)
    if (!Number.isSafeInteger(b.sourceFrame) || !Number.isSafeInteger(b.targetFrame) ||
      a.sourceFrame >= b.sourceFrame || a.targetFrame >= b.targetFrame ||
      ratio < 0.25 || ratio > 4)
      return t("Warp markers must remain ordered, with each time ratio between 0.25 and 4.")
  }
  for (const edit of recipe.overrides) {
    const note = recipe.analysis.notes[edit.note]
    if (!note || !Number.isFinite(edit.midiPitch) || edit.midiPitch < 0 || edit.midiPitch > 127 ||
      Math.abs(edit.midiPitch - note.midiPitch) > 24 ||
      !Number.isSafeInteger(edit.startFrame) || !Number.isSafeInteger(edit.endFrame) ||
      edit.startFrame < 0 || edit.endFrame > output || edit.startFrame >= edit.endFrame ||
      (edit.endFrame - edit.startFrame) / (note.endFrame - note.startFrame) < 0.25 ||
      (edit.endFrame - edit.startFrame) / (note.endFrame - note.startFrame) > 4)
      return t("Note edits need valid ordered frames and at most 24 semitones of correction.")
  }
  if ((recipe.analysis.polyphonic && recipe.overrides.length > 0) ||
    new Set(recipe.overrides.map((o) => o.note)).size !== recipe.overrides.length)
    return t("Pitch overrides must be unique and monophonic.")
  const editedEdges = new Map<number, number>()
  for (const edit of recipe.overrides) {
    const note = recipe.analysis.notes[edit.note]
    for (const [source, target] of [[note.startFrame, edit.startFrame], [note.endFrame, edit.endFrame]]) {
      if ((source === 0 && target !== 0) || (source === recipe.analysis.frames && target !== output))
        return t("Note edits cannot move fixed render endpoints.")
      if (editedEdges.has(source) && editedEdges.get(source) !== target)
        return t("Adjacent edited notes must agree on their shared boundary.")
      editedEdges.set(source, target)
    }
  }
  const combined = renderMarkers(recipe)
  for (let i = 1; i < combined.length; ++i) {
    const a = combined[i - 1], b = combined[i]
    const ratio = (b.targetFrame - a.targetFrame) / (b.sourceFrame - a.sourceFrame)
    if (a.targetFrame >= b.targetFrame || ratio < 0.25 || ratio > 4)
      return t("Combined note and warp anchors must remain ordered with ratios from 0.25 to 4.")
  }
  if (!recipe.analysis.polyphonic) {
    const notes = recipe.analysis.notes.map((_, i) => noteEdit(recipe, i))
    if (notes.some((n, i) => i > 0 && notes[i - 1].endFrame > n.startFrame))
      return t("Edited notes cannot overlap or change order.")
  }
  if (recipe.denoise) {
    const d = recipe.denoise
    if (!Number.isSafeInteger(d.profileStart) || !Number.isSafeInteger(d.profileEnd) ||
      d.profileStart < 0 || d.profileStart >= d.profileEnd || d.profileEnd > recipe.analysis.frames ||
      !Number.isFinite(d.reductionDb) || d.reductionDb < 0 || d.reductionDb > 40 ||
      !Number.isFinite(d.wet) || d.wet < 0 || d.wet > 1)
      return t("Select a noise-only profile with valid attenuation and wet mix.")
  }
  return null
}
