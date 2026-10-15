import { describe, expect, it } from "vitest"

import fixtures from "../../../../crates/windfall-project/tests/timeline/meter-fixtures.json"
import { formatPositionText, parsePositionText } from "./position-text"
import { MAX_SONG_TICKS } from "./units"

const common = { numerator: 4, denominator: 4 }

describe("song positions as text", () => {
  it("shows bar and beat, and the tick only off the beat", () => {
    expect(formatPositionText(0, common)).toBe("1:1")
    expect(formatPositionText(3840 * 8, common)).toBe("9:1")
    expect(formatPositionText(3840 * 8 + 960 + 480, common)).toBe("9:2:480")
  })

  it("reads a bar, a bar and beat, or a full position", () => {
    expect(parsePositionText("9", common)).toBe(3840 * 8)
    expect(parsePositionText(" 9:2 ", common)).toBe(3840 * 8 + 960)
    expect(parsePositionText("9.2.480", common)).toBe(3840 * 8 + 960 + 480)
    expect(parsePositionText("1:1:0", common)).toBe(0)
  })

  it("rejects text that is not a position in the song", () => {
    for (const text of ["", "0", "1:0", "1:5", "1:1:960", "-1", "9:x", "1:2:3:4"])
      expect(parsePositionText(text, common), text).toBeNull()
    expect(
      parsePositionText(String(MAX_SONG_TICKS / 3840 + 2), common)
    ).toBeNull()
  })

  it("follows meter changes and round-trips every shared fixture tick", () => {
    for (const entry of fixtures.positions) {
      const text = formatPositionText(
        entry.tick,
        fixtures.legacy,
        fixtures.meters
      )
      expect(
        parsePositionText(text, fixtures.legacy, fixtures.meters),
        text
      ).toBe(entry.tick)
    }
  })
})
