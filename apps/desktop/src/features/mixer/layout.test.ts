import { describe, expect, it } from "vitest"

import {
  ADAPTIVE_MAX_WIDTH,
  ADD_WIDTH,
  adaptiveStripWidth,
  EFFECT_ROW_HEIGHT,
  INSPECTOR_MIN_WIDTH,
  INSPECTOR_WIDTH,
  inspectorWidthFor,
  MAX_EFFECT_ROWS,
  MAX_SEND_ROWS,
  MIN_EFFECT_ROWS,
  scrollToReveal,
  SEND_ROW_HEIGHT,
  STRIP_WIDTH,
  stripLayout,
  visibleRange,
} from "./layout"

describe("the width the effects start at", () => {
  it("is wide enough for an editor's wide layout when the window has room", () => {
    expect(inspectorWidthFor(1440)).toBe(INSPECTOR_WIDTH)
    expect(inspectorWidthFor(1920)).toBe(INSPECTOR_WIDTH)
    // An editor lays its controls beside its display from 26rem, and the
    // panel around it takes 16 pixels.
    expect(INSPECTOR_WIDTH - 16).toBeGreaterThanOrEqual(26 * 16)
  })

  it("leaves most of the mixer to the strips in a small window", () => {
    expect(inspectorWidthFor(960)).toBe(346)
    expect(inspectorWidthFor(960)).toBeLessThan(960 / 2)
    expect(inspectorWidthFor(640)).toBe(INSPECTOR_MIN_WIDTH)
  })
})

describe("adaptiveStripWidth", () => {
  it("spreads a few strips over the room so their names stay readable", () => {
    // Five tracks in 800 pixels are not squeezed to the standard width.
    expect(adaptiveStripWidth(800, 5)).toBe(ADAPTIVE_MAX_WIDTH)
    expect(adaptiveStripWidth(600, 5)).toBe(Math.floor((600 - ADD_WIDTH) / 5))
  })

  it("falls back to the standard width for many strips or no measurement", () => {
    expect(adaptiveStripWidth(800, 40)).toBe(STRIP_WIDTH)
    expect(adaptiveStripWidth(0, 5)).toBe(STRIP_WIDTH)
    expect(adaptiveStripWidth(800, 0)).toBe(STRIP_WIDTH)
  })
})

describe("stripLayout", () => {
  const mode = (height: number, sends = 0) => stripLayout(height, sends).mode

  // The lowest panel that shows everything inline with no sends and no
  // effects: the rack is two rows then.
  const FULL = 335

  it("shows everything inline when the panel is tall", () => {
    expect(stripLayout(460, 0)).toEqual({
      mode: "full",
      sendRows: 0,
      effectRows: MIN_EFFECT_ROWS,
    })
    expect(mode(FULL)).toBe("full")
  })

  it("moves the routing into a popover when the fader would get short", () => {
    expect(mode(FULL - 1)).toBe("compact")
    expect(mode(238)).toBe("compact")
  })

  it("drops the chips and the peak readout to keep the fader upright", () => {
    expect(mode(237)).toBe("tight")
    expect(mode(196)).toBe("tight")
  })

  it("lays the fader flat at the lowest heights", () => {
    expect(mode(195)).toBe("flat")
    expect(mode(130)).toBe("flat")
    expect(mode(129)).toBe("mini")
    expect(mode(80, 4)).toBe("mini")
  })

  it("asks for more height before it shows send rows inline", () => {
    expect(stripLayout(FULL + SEND_ROW_HEIGHT, 1)).toMatchObject({
      mode: "full",
      sendRows: 1,
    })
    expect(stripLayout(FULL + SEND_ROW_HEIGHT - 1, 1)).toMatchObject({
      mode: "compact",
      sendRows: 0,
    })
  })

  it("makes the rack one row taller than the longest chain", () => {
    expect(stripLayout(900, 0, 0).effectRows).toBe(MIN_EFFECT_ROWS)
    expect(stripLayout(900, 0, 1).effectRows).toBe(2)
    expect(stripLayout(900, 0, 2).effectRows).toBe(3)
    expect(stripLayout(900, 0, 10).effectRows).toBe(MAX_EFFECT_ROWS)
  })

  it("asks for more height before it shows a longer chain in full", () => {
    const taller = FULL + 2 * EFFECT_ROW_HEIGHT
    expect(stripLayout(taller, 0, 3)).toMatchObject({
      mode: "full",
      effectRows: 4,
    })
    expect(stripLayout(taller - 1, 0, 3)).toMatchObject({
      mode: "compact",
      effectRows: 4,
    })
  })

  it("gives the rack what the shortest fader leaves, and scrolls the rest", () => {
    expect(stripLayout(238, 0, 10)).toMatchObject({
      mode: "compact",
      effectRows: 2,
    })
    expect(stripLayout(238 + EFFECT_ROW_HEIGHT, 0, 10)).toMatchObject({
      mode: "compact",
      effectRows: 3,
    })
    expect(stripLayout(330, 0, 10).effectRows).toBe(MAX_EFFECT_ROWS)
    // The upright fader of the tight layout gets the room instead.
    expect(stripLayout(237, 0, 10)).toMatchObject({
      mode: "tight",
      effectRows: MIN_EFFECT_ROWS,
    })
  })

  it("turns the rack into a badge when the fader lies flat", () => {
    expect(stripLayout(195, 0, 6).effectRows).toBe(0)
    expect(stripLayout(100, 0, 6).effectRows).toBe(0)
  })

  it("never reserves more than a few send rows", () => {
    expect(stripLayout(900, 12).sendRows).toBe(MAX_SEND_ROWS)
  })

  it("uses the full layout until the panel has been measured", () => {
    expect(stripLayout(0, 2)).toEqual({
      mode: "full",
      sendRows: 2,
      effectRows: MIN_EFFECT_ROWS,
    })
  })
})

describe("visibleRange", () => {
  it("mounts the strips in view and two on each side", () => {
    // Strips 10 to 14 are in view.
    expect(visibleRange(10 * STRIP_WIDTH, 5 * STRIP_WIDTH, 127)).toEqual({
      start: 8,
      end: 17,
    })
  })

  it("stops at both ends of the mixer", () => {
    expect(visibleRange(0, 5 * STRIP_WIDTH, 127)).toEqual({ start: 0, end: 7 })
    expect(visibleRange(125 * STRIP_WIDTH, 5 * STRIP_WIDTH, 127)).toEqual({
      start: 123,
      end: 127,
    })
  })

  it("mounts every strip of a short mixer", () => {
    expect(visibleRange(0, 1200, 4)).toEqual({ start: 0, end: 4 })
    expect(visibleRange(0, 1200, 0)).toEqual({ start: 0, end: 0 })
  })

  it("counts a strip that is only partly in view", () => {
    expect(visibleRange(STRIP_WIDTH / 2, STRIP_WIDTH, 127)).toEqual({
      start: 0,
      end: 4,
    })
  })

  it("stays far below the track count however long the mixer is", () => {
    const range = visibleRange(40 * STRIP_WIDTH, 1200, 127)
    expect(range.end - range.start).toBeLessThanOrEqual(20)
  })
})

describe("scrollToReveal", () => {
  const view = 5 * STRIP_WIDTH

  it("leaves a strip that is fully in view alone", () => {
    expect(scrollToReveal(2, 0, view)).toBeNull()
    expect(scrollToReveal(4, 0, view)).toBeNull()
  })

  it("scrolls back to a strip on the left", () => {
    expect(scrollToReveal(3, 10 * STRIP_WIDTH, view)).toBe(3 * STRIP_WIDTH)
  })

  it("scrolls on just far enough for a strip on the right", () => {
    expect(scrollToReveal(5, 0, view)).toBe(STRIP_WIDTH)
    expect(scrollToReveal(20, 0, view)).toBe(16 * STRIP_WIDTH)
  })

  it("brings a strip that is cut off fully into view", () => {
    expect(scrollToReveal(5, 30, view)).toBe(STRIP_WIDTH)
    expect(scrollToReveal(0, 30, view)).toBe(0)
  })
})
