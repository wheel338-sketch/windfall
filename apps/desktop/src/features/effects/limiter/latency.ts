import { message, plainNumber } from "@/lib/i18n"
import type { EffectSlot } from "@/bindings"

/**
 * The frames a limiter delays its signal by: its look-ahead at the sample
 * rate, and never less than one frame. This is `latency_samples` in
 * `crates/windfall-dsp/src/limiter.rs`.
 */
export function lookaheadFrames(lookaheadMs: number, sampleRate: number) {
  return Math.max(1, Math.round((lookaheadMs * Math.max(1, sampleRate)) / 1000))
}

/**
 * Shared latency of built-in slots, including bypassed and dry slots.
 * Matrix channel differences remain intentional stereo processing.
 */
export function chainLatencyFrames(
  effects: readonly EffectSlot[],
  sampleRate: number
): number {
  let frames = 0
  for (const slot of effects) {
    if (slot.params.type === "limiter") {
      frames += lookaheadFrames(slot.params.lookaheadMs, sampleRate)
    } else if (slot.params.type === "stereoMatrix") {
      const rate = Math.min(384_000, Math.max(1, sampleRate))
      // Match the native f32 multiplication order before sample rounding.
      const delay = (ms: number) =>
        Math.round(
          Math.fround(
            Math.fround(Math.fround(ms) * Math.fround(0.001)) *
              Math.fround(rate)
          )
        )
      frames += Math.min(
        delay(slot.params.leftDelayMs),
        delay(slot.params.rightDelayMs)
      )
    } else if (slot.params.type === "distortion") {
      frames += 32
    }
  }
  return frames
}

/** Frames as milliseconds at a sample rate. */
export function framesToMs(frames: number, sampleRate: number): number {
  return (frames * 1000) / Math.max(1, sampleRate)
}

/** "5.0 ms (240 samples)". */
export function formatLatency(frames: number, sampleRate: number): string {
  const ms = framesToMs(frames, sampleRate)
  return message("{ms} ms ({shown} samples)", { ms: ms.toFixed(ms < 10 ? 1 : 0), count: frames, shown: plainNumber(frames) })
}
