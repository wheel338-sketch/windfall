import { describe, expect, it } from "vitest"

import type { SamplerChannel } from "@/lib/channel-source"

import {
  hasLoopSeam,
  isTrimmed,
  NO_PREPROCESSING,
  preprocessRequest,
} from "./preprocess"

function sampler(source: Partial<SamplerChannel["source"]> = {}) {
  return {
    id: 7,
    name: "Loop",
    color: 0,
    volume: 1,
    pan: 0,
    muted: false,
    solo: false,
    mixerTrack: 0,
    source: {
      type: "sampler",
      sample: 3,
      rootKey: 60,
      tune: 0,
      gain: 1,
      start: 0,
      end: 1,
      reverse: false,
      envelope: null,
      cutSelf: false,
      cutGroup: 0,
      ...source,
    },
  } as SamplerChannel
}

describe("sample pre-processing requests", () => {
  it("sends only the chosen treatments", () => {
    const built = preprocessRequest(sampler(), {
      ...NO_PREPROCESSING,
      reverse: true,
      normalize: true,
      fadeOutMs: "12.5",
    })
    expect(built).toEqual({
      request: { channel: 7, reverse: true, fadeOutMs: 12.5, normalize: true },
    })
  })

  it("refuses an empty choice, a missing sample and a bad fade", () => {
    expect(preprocessRequest(sampler(), NO_PREPROCESSING)).toEqual({
      error: "Choose at least one treatment to apply.",
    })
    expect(
      preprocessRequest(sampler({ sample: null }), {
        ...NO_PREPROCESSING,
        normalize: true,
      })
    ).toEqual({ error: "Load a sample into the channel first." })
    for (const fadeInMs of ["-1", "abc", "60001", "Infinity"])
      expect(
        preprocessRequest(sampler(), { ...NO_PREPROCESSING, fadeInMs })
      ).toEqual({ error: "A fade is a time from 0 to 60000 ms." })
  })

  it("offers the loop seam only for a forward loop and caps it at half", () => {
    const choices = { ...NO_PREPROCESSING, loopCrossfade: 0.9 }
    expect(hasLoopSeam(sampler())).toBe(false)
    expect(hasLoopSeam(sampler({ loopMode: "pingPong" }))).toBe(false)
    // Without a seam the crossfade is not a treatment at all.
    expect(preprocessRequest(sampler(), choices)).toEqual({
      error: "Choose at least one treatment to apply.",
    })
    expect(preprocessRequest(sampler({ loopMode: "forward" }), choices)).toEqual(
      { request: { channel: 7, loopCrossfade: 0.5 } }
    )
  })

  it("asks for a trim only when the region is shorter than the sample", () => {
    const choices = { ...NO_PREPROCESSING, trim: true }
    expect(isTrimmed(sampler())).toBe(false)
    expect(preprocessRequest(sampler(), choices)).toEqual({
      error: "Choose at least one treatment to apply.",
    })
    expect(isTrimmed(sampler({ end: 0.5 }))).toBe(true)
    expect(preprocessRequest(sampler({ start: 0.25 }), choices)).toEqual({
      request: { channel: 7, trim: true },
    })
  })
})
