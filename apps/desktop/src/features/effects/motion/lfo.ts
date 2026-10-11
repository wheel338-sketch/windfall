import type { LfoShape } from "@/bindings"

/*
 * The pan and volume motion, as the engine computes it
 * (`crates/windfall-dsp/src/control/motion.rs`), for the editor's picture
 * of one cycle.
 */

/** Levels the picture gives the random shape, which the engine draws anew. */
const RANDOM_PREVIEW = [0.6, -0.8, 0.2, -0.3, 0.9, -0.6, 0.4, -0.1]

/**
 * The wave at `phase` cycles, from -1 to 1. Every shape but the saw starts
 * at or above 0. The random shape holds one level per cycle; the levels
 * here only stand for it.
 */
export function lfoWave(shape: LfoShape, phase: number): number {
  const at = phase - Math.floor(phase)
  switch (shape) {
    case "sine":
      return Math.sin(2 * Math.PI * at)
    case "triangle":
      return at < 0.25 ? 4 * at : at < 0.75 ? 2 - 4 * at : 4 * at - 4
    case "saw":
      return 2 * at - 1
    case "square":
      return at < 0.5 ? 1 : -1
    case "random": {
      const cycle = Math.floor(phase)
      const count = RANDOM_PREVIEW.length
      return RANDOM_PREVIEW[((cycle % count) + count) % count]
    }
  }
}

export type MotionSettings = {
  pan: number
  depth: number
  gain: number
  volumeDepth: number
}

/** The pan a wave value gives: the center moved by the depth, inside L to R. */
export function motionPan(settings: MotionSettings, wave: number): number {
  return Math.min(1, Math.max(-1, settings.pan + settings.depth * wave))
}

/** The gain a wave value gives: full at the bottom of the wave. */
export function motionGain(settings: MotionSettings, wave: number): number {
  return settings.gain * (1 - settings.volumeDepth * (0.5 + 0.5 * wave))
}

/** Seconds one cycle takes: the tempo's while synced, the rate's otherwise. */
export function cycleSeconds(
  timing: { sync: boolean; beats: number; rateHz: number },
  tempoBpm: number
): number {
  const hz = timing.sync
    ? Math.max(1, tempoBpm) / 60 / timing.beats
    : timing.rateHz
  return 1 / hz
}

/** The lowest and highest pan the motion reaches. */
export function panReach(settings: MotionSettings): [number, number] {
  return [motionPan(settings, -1), motionPan(settings, 1)]
}
