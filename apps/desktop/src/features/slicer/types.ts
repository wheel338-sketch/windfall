import type { Clip, ClipId } from "@/bindings"

/** Wire contract of windfall-project::slicer and windfall-ipc::SliceReview. */
export type SliceOptions =
  | { mode: "grid"; gridTicks: number }
  | { mode: "transients"; sensitivity: number }
  | { mode: "wavMarkers" }
export type SliceAnalysis = {
  markers: { tick: number; strength: number }[]
  peaks: number[]
}
export type SliceReview = {
  token: number
  clip: ClipId
  lengthTicks: number
  analysis: SliceAnalysis
}
export type SliceAudio = {
  sampleRate: number
  channels: number
  samples: number[]
}
export type SliceAnalysisRequest = SliceAudio & {
  clip: Clip
  tempo: number
  swing: number
  options: SliceOptions
}
