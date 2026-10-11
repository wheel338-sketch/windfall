import { afterEach, describe, expect, it, vi } from "vitest"

import { announceProjectReplaced } from "@/lib/store/replaced"

import {
  heldKeys,
  hardwareHeldKey,
  holdKey,
  releaseHeldKeys,
  releaseHardwareKeys,
  releaseKey,
  subscribeHeldKeys,
} from "./held-keys"

afterEach(() => releaseHeldKeys())

describe("held keys", () => {
  it("clears hardware ownership on panic while an independent audition survives", () => {
    holdKey(1, 60)
    hardwareHeldKey(1, 60, 100)
    hardwareHeldKey(1, 64, 100)
    releaseHardwareKeys()
    expect(heldKeys(1)).toEqual([60])
    releaseKey(1, 60)
    expect(heldKeys(1)).toEqual([])
  })
  it("keeps sounding hardware keys through sustain, repeated down and independent audition releases", () => {
    hardwareHeldKey(1, 60, 100)
    hardwareHeldKey(1, 60, 90)
    holdKey(1, 60)
    releaseKey(1, 60)
    // Physical key-up under sustain does not produce an accepted sounding up.
    expect(heldKeys(1)).toEqual([60])
    hardwareHeldKey(2, 67, 100)
    expect(heldKeys(1)).toEqual([60])
    hardwareHeldKey(1, 60, 0)
    expect(heldKeys(1)).toEqual([])
    expect(heldKeys(2)).toEqual([67])
    hardwareHeldKey(1, 128, 100)
    hardwareHeldKey(1, -1, 100)
    hardwareHeldKey(1, 60, NaN)
    expect(heldKeys(1)).toEqual([])
    announceProjectReplaced()
    expect(heldKeys(2)).toEqual([])
  })
  it("lists a channel's held keys in ascending order and keeps channels apart", () => {
    holdKey(1, 67)
    holdKey(1, 60)
    holdKey(1, 64)
    holdKey(2, 72)
    expect(heldKeys(1)).toEqual([60, 64, 67])
    expect(heldKeys(2)).toEqual([72])
    expect(heldKeys(3)).toEqual([])
    expect(heldKeys(null)).toEqual([])
    expect(heldKeys(undefined)).toEqual([])
  })

  it("counts a key once and lets it go on the first release", () => {
    const listener = vi.fn()
    const stop = subscribeHeldKeys(listener)
    holdKey(1, 60)
    holdKey(1, 60)
    expect(heldKeys(1)).toEqual([60])
    expect(listener).toHaveBeenCalledTimes(1)
    releaseKey(1, 60)
    expect(heldKeys(1)).toEqual([])
    expect(listener).toHaveBeenCalledTimes(2)
    // A release for a key that is not held changes nothing.
    releaseKey(1, 60)
    releaseKey(9, 60)
    expect(listener).toHaveBeenCalledTimes(2)
    stop()
    holdKey(1, 61)
    expect(listener).toHaveBeenCalledTimes(2)
  })

  it("hands back the same array until that channel changes", () => {
    holdKey(1, 60)
    holdKey(2, 70)
    const first = heldKeys(1)
    expect(heldKeys(1)).toBe(first)
    holdKey(2, 71)
    releaseKey(2, 70)
    expect(heldKeys(1)).toBe(first)
    holdKey(1, 64)
    expect(heldKeys(1)).not.toBe(first)
    expect(heldKeys(1)).toEqual([60, 64])
    releaseHeldKeys(1)
    expect(heldKeys(1)).toBe(heldKeys(99))
  })

  it("ignores keys that are not whole numbers", () => {
    const listener = vi.fn()
    const stop = subscribeHeldKeys(listener)
    holdKey(1, NaN)
    holdKey(1, 60.5)
    expect(heldKeys(1)).toEqual([])
    expect(listener).not.toHaveBeenCalled()
    stop()
  })

  it("releases one channel, or every channel, at once", () => {
    const listener = vi.fn()
    const stop = subscribeHeldKeys(listener)
    holdKey(1, 60)
    holdKey(1, 64)
    holdKey(2, 67)
    listener.mockClear()
    releaseHeldKeys(1)
    expect(heldKeys(1)).toEqual([])
    expect(heldKeys(2)).toEqual([67])
    expect(listener).toHaveBeenCalledTimes(1)
    releaseHeldKeys(1)
    expect(listener).toHaveBeenCalledTimes(1)
    holdKey(1, 62)
    listener.mockClear()
    releaseHeldKeys()
    expect(heldKeys(1)).toEqual([])
    expect(heldKeys(2)).toEqual([])
    expect(listener).toHaveBeenCalledTimes(1)
    releaseHeldKeys()
    expect(listener).toHaveBeenCalledTimes(1)
    stop()
  })

  it("releases everything when the project is replaced", () => {
    const listener = vi.fn()
    const stop = subscribeHeldKeys(listener)
    holdKey(1, 60)
    holdKey(4, 64)
    listener.mockClear()
    announceProjectReplaced()
    expect(heldKeys(1)).toEqual([])
    expect(heldKeys(4)).toEqual([])
    expect(listener).toHaveBeenCalledTimes(1)
    stop()
  })
})
