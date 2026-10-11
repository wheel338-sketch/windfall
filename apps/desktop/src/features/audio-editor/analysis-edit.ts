import type { AudioEditRecipe, AudioNoteOverride } from "./analysis-types"

export function mappedFrame(recipe: AudioEditRecipe, source: number): number {
  const markers = recipe.markers
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
    return "Keep source endpoints and the origin fixed."
  const output = markers.at(-1)!.targetFrame
  if (!Number.isSafeInteger(output) || output <= 0 ||
    output / recipe.analysis.sampleRate > 120)
    return "Output duration must fit within two minutes."
  for (let i = 1; i < markers.length; ++i) {
    const a = markers[i - 1], b = markers[i]
    const ratio = (b.targetFrame - a.targetFrame) / (b.sourceFrame - a.sourceFrame)
    if (!Number.isSafeInteger(b.sourceFrame) || !Number.isSafeInteger(b.targetFrame) ||
      a.sourceFrame >= b.sourceFrame || a.targetFrame >= b.targetFrame ||
      ratio < 0.25 || ratio > 4)
      return "Warp markers must remain ordered, with each time ratio between 0.25 and 4."
  }
  for (const edit of recipe.overrides) {
    const note = recipe.analysis.notes[edit.note]
    if (!note || !Number.isFinite(edit.midiPitch) || edit.midiPitch < 0 || edit.midiPitch > 127 ||
      Math.abs(edit.midiPitch - note.midiPitch) > 24 ||
      !Number.isSafeInteger(edit.startFrame) || !Number.isSafeInteger(edit.endFrame) ||
      edit.startFrame < 0 || edit.endFrame > output || edit.startFrame >= edit.endFrame ||
      (edit.endFrame - edit.startFrame) / (note.endFrame - note.startFrame) < 0.25 ||
      (edit.endFrame - edit.startFrame) / (note.endFrame - note.startFrame) > 4)
      return "Note edits need valid ordered frames and at most 24 semitones of correction."
  }
  if (!recipe.analysis.polyphonic) {
    const notes = recipe.analysis.notes.map((_, i) => noteEdit(recipe, i))
    if (notes.some((n, i) => i > 0 && notes[i - 1].endFrame > n.startFrame))
      return "Edited notes cannot overlap or change order."
  }
  if (recipe.denoise) {
    const d = recipe.denoise
    if (d.profileStart >= d.profileEnd || d.profileEnd > recipe.analysis.frames ||
      !Number.isFinite(d.reductionDb) || d.reductionDb < 0 || d.reductionDb > 40 ||
      !Number.isFinite(d.wet) || d.wet < 0 || d.wet > 1)
      return "Select a noise-only profile with valid attenuation and wet mix."
  }
  return null
}
