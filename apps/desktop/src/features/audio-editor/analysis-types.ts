import type { Clip } from "@/bindings"

/** Mirrors project/audio_analysis.rs; generated bindings are integrator-owned. */
export type DetectedAudioNote = {
  startFrame: number
  endFrame: number
  midiPitch: number
  confidence: number
  rms: number
}
export type AudioNoteOverride = {
  note: number
  midiPitch: number
  startFrame: number
  endFrame: number
}
export type AudioWarpMarker = { sourceFrame: number; targetFrame: number }
export type AudioViewTiming = {
  durationFrames: number
  offsetFrames: number
  fadeInFrames: number
  fadeOutFrames: number
}
export type AudioEditRecipe = {
  sourceClip: Clip
  sourceTiming?: AudioViewTiming | null
  analysis: {
    frames: number
    sampleRate: number
    polyphonic: boolean
    notes: DetectedAudioNote[]
    transients: number[]
  }
  overrides: AudioNoteOverride[]
  markers: AudioWarpMarker[]
  denoise: {
    profileStart: number
    profileEnd: number
    reductionDb: number
    wet: number
  } | null
}
export type AudioAnalysisEdit = { token: number; recipe: AudioEditRecipe }
export type AudioAnalysisPreview = AudioAnalysisEdit
