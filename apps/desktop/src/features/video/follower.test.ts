import { describe, expect, it, vi } from "vitest"
import type { MediaClock } from "@/bindings"
import { TempoMap } from "@/lib/automation/tempo-map"
import { VideoFollower, videoTarget, type MediaLike } from "./follower"

const clock = (patch: Partial<MediaClock> = {}): MediaClock => ({
  epoch: "1", outputFrame: "9007199254740993", sampleRate: 48000,
  pdcFrames: 144, deviceLatencyFrames: null, playing: true, mode: "song", tick: 0, ...patch,
})
const media = (): MediaLike => ({ currentTime: 0, playbackRate: 1, paused: true, readyState: 4, duration: 100, play: vi.fn(() => Promise.resolve()), pause: vi.fn() })

describe("audio-clock video follower", () => {
  it("maps clip offsets against musical tempo and stops outside the half-open placement", () => {
    const placement = { path: "C:/own.webm", startTick: 960, lengthTicks: 1920, offsetSeconds: 2, muted: true, mixerTrack: 0 }
    const normal = new TempoMap(null, 120, 0); const slow = new TempoMap(null, 60, 0)
    expect(videoTarget(placement, clock({ tick: 1920 }), (tick) => normal.secondsAt(tick))).toBe(2.5)
    expect(videoTarget(placement, clock({ tick: 1920, epoch: "2" }), (tick) => slow.secondsAt(tick))).toBe(3)
    expect(videoTarget(placement, clock({ tick: 960 }), (tick) => normal.secondsAt(tick))).toBe(2)
    expect(videoTarget(placement, clock({ tick: 959 }), (tick) => normal.secondsAt(tick))).toBeNull()
    expect(videoTarget(placement, clock({ tick: 2880 }), (tick) => normal.secondsAt(tick))).toBeNull()
    expect(videoTarget(placement, clock({ tick: 1920, mode: "pattern" }), (tick) => normal.secondsAt(tick))).toBeNull()
  })
  it("seeks on epoch changes and loops without reading a wall clock", () => {
    const video = media(); const follower = new VideoFollower()
    expect(follower.follow(video, clock(), 12).correction).toBe("seek")
    expect(video.currentTime).toBe(12)
    video.currentTime = 12.01
    expect(follower.follow(video, clock({ outputFrame: "9007199254740994" }), 12.02).correction).toBe("locked")
    expect(follower.follow(video, clock({ epoch: "2" }), 12.03).correction).toBe("seek")
    expect(follower.follow(video, clock({ outputFrame: "9007199254740995" }), 2).correction).toBe("seek")
    expect(video.currentTime).toBe(2)
    expect(video.play).not.toHaveBeenCalled()
  })

  it("trims modest drift, hard-corrects large drift and pauses a stalled sample cursor", () => {
    const video = media(); const follower = new VideoFollower()
    follower.follow(video, clock(), 1)
    video.currentTime = 1
    expect(follower.follow(video, clock({ outputFrame: "2" }), 1.08).correction).toBe("rate")
    expect(video.playbackRate).toBeCloseTo(1.04)
    video.currentTime = 1.3
    expect(follower.follow(video, clock({ outputFrame: "3" }), 1.09).correction).toBe("seek")
    expect(video.currentTime).toBe(1.09)
    for (let i = 0; i < 5; i += 1) expect(follower.follow(video, clock({ outputFrame: "3" }), 1.09).correction).toBe("locked")
    expect(follower.follow(video, clock({ outputFrame: "3" }), 1.09).correction).toBe("stalled")
    expect(video.pause).toHaveBeenCalled()
  })

  it("holds exact paused seeks and refuses absent, undecoded or out-of-range media", () => {
    const video = media(); const follower = new VideoFollower()
    expect(follower.follow(video, clock({ playing: false }), 4).correction).toBe("paused")
    expect(video.currentTime).toBe(4)
    expect(follower.follow(video, null, 5).correction).toBe("waiting")
    expect(follower.follow(video, clock(), null).correction).toBe("waiting")
    video.readyState = 0
    expect(follower.follow(video, clock(), 5).correction).toBe("waiting")
    video.readyState = 4
    expect(follower.follow(video, clock(), 100).correction).toBe("waiting")
    follower.reset()
    expect(follower.follow(video, clock({ outputFrame: "4" }), 4).correction).toBe("seek")
  })
})
