import type { PlaybackCurve, SamplePlayback, SliceRegion } from "@/bindings"

export function defaultRegion(): SliceRegion {
  return { start: 0, end: 1, gain: 1, pitch: 0, reverse: false, attackMs: 1, releaseMs: 4, deck: 0 }
}
export function defaultCurve(): PlaybackCurve {
  return { mode: "position", origin: 0, points: [{ time: 0, value: 0 }, { time: 1, value: 1 }] }
}
export function defaultAsset(): SamplePlayback {
  return { sample: null, sampleB: null, slices: [defaultRegion()], curve: defaultCurve(), keyCurves: [], sourceBpm: 120, sourceBeats: 4 }
}
