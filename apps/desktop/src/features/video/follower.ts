import type { MediaClock } from "@/bindings"

export type MediaLike = {
  currentTime: number
  playbackRate: number
  paused: boolean
  readyState: number
  duration: number
  play(): Promise<void>
  pause(): void
}
export type FollowResult = { drift: number; correction: "waiting" | "paused" | "seek" | "rate" | "locked" | "stalled" }

/** Uses only audio presentation samples, never elapsed wall time. The media
 * element's own decoder does the frame dropping and seeking. */
export class VideoFollower {
  private epoch: string | null = null
  private lastTarget: number | null = null
  private outputFrame: string | null = null
  private repeats = 0

  reset() { this.epoch = null; this.lastTarget = null; this.outputFrame = null; this.repeats = 0 }

  follow(media: MediaLike, clock: MediaClock | null | undefined, target: number | null): FollowResult {
    if (!clock || target === null || !Number.isFinite(target)) {
      media.pause(); this.reset()
      return { drift: 0, correction: "waiting" }
    }
    this.repeats = clock.outputFrame === this.outputFrame ? this.repeats + 1 : 0
    this.outputFrame = clock.outputFrame
    const drift = target - media.currentTime
    const jump = this.epoch !== clock.epoch || (this.lastTarget !== null && (target < this.lastTarget - 0.01 || Math.abs(target - this.lastTarget) > 0.25))
    this.epoch = clock.epoch; this.lastTarget = target
    if (media.readyState < 1 || (Number.isFinite(media.duration) && target >= media.duration)) {
      media.pause()
      return { drift, correction: "waiting" }
    }
    if (jump || Math.abs(drift) > 0.12) {
      media.currentTime = Math.max(0, target)
      media.playbackRate = 1
    } else {
      media.playbackRate = Math.abs(drift) > 0.02 ? Math.max(0.95, Math.min(1.05, 1 + drift * 0.5)) : 1
    }
    if (!clock.playing || this.repeats >= 6) {
      media.pause()
      return { drift, correction: clock.playing ? "stalled" : "paused" }
    }
    // A play rejection is handled by the mounted view, not silently ignored.
    return { drift, correction: jump || Math.abs(drift) > 0.12 ? "seek" : media.playbackRate === 1 ? "locked" : "rate" }
  }
}
