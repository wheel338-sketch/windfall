import { describe, expect, it } from "vitest"

import { cursorFor, intentFor, TOOLS, type Press, type Tool } from "./intents"

const press = (overrides: Partial<Press> = {}): Press => ({
  tool: "draw",
  button: 0,
  mod: false,
  shift: false,
  hit: null,
  ...overrides,
})

const body = { id: 7, part: "body" } as const
const startEdge = { id: 7, part: "start-edge" } as const
const endEdge = { id: 7, part: "end-edge" } as const

describe("intentFor", () => {
  it("places a clip with the Draw tool on empty grid", () => {
    expect(intentFor(press())).toEqual({ kind: "place" })
  })

  it("paints with the Paint tool on empty grid", () => {
    expect(intentFor(press({ tool: "paint" }))).toEqual({ kind: "paint" })
  })

  it("selects with a box in the Select tool, Shift adding to the selection", () => {
    expect(intentFor(press({ tool: "select" }))).toEqual({
      kind: "marquee",
      additive: false,
    })
    expect(intentFor(press({ tool: "select", shift: true }))).toEqual({
      kind: "marquee",
      additive: true,
    })
  })

  it.each<Tool>(["draw", "paint", "select"])(
    "moves and resizes a pressed clip with the %s tool",
    (tool) => {
      expect(intentFor(press({ tool, hit: body }))).toEqual({
        kind: "move",
        id: 7,
        additive: false,
      })
      expect(intentFor(press({ tool, hit: body, shift: true }))).toEqual({
        kind: "move",
        id: 7,
        additive: true,
      })
      expect(intentFor(press({ tool, hit: startEdge }))).toEqual({
        kind: "trim-start",
        id: 7,
      })
      expect(intentFor(press({ tool, hit: endEdge }))).toEqual({
        kind: "resize-end",
        id: 7,
      })
    }
  )

  it("erases and mutes with their tools, on a clip or beside one", () => {
    expect(intentFor(press({ tool: "erase", hit: body }))).toEqual({
      kind: "erase",
    })
    expect(intentFor(press({ tool: "erase" }))).toEqual({ kind: "erase" })
    expect(intentFor(press({ tool: "mute", hit: endEdge }))).toEqual({
      kind: "mute",
    })
  })

  it("slips a clip on its body or edges, leaving empty grid alone", () => {
    for (const hit of [body, startEdge, endEdge]) {
      expect(intentFor(press({ tool: "slip", hit }))).toEqual({
        kind: "slip",
        id: 7,
      })
    }
    expect(intentFor(press({ tool: "slip" }))).toEqual({ kind: "none" })
  })

  it("slips over audio and automation handles without editing their content", () => {
    for (const inner of [
      { kind: "fade", edge: "in" },
      { kind: "gain" },
      { kind: "point", index: 0 },
      { kind: "bend", index: 0 },
      { kind: "curve" },
    ] as const) {
      expect(intentFor(press({ tool: "slip", hit: body, inner }))).toEqual({
        kind: "slip",
        id: 7,
      })
      expect(cursorFor("slip", "body", inner)).toBe("ew-resize")
    }
  })

  it("keeps Ctrl and Shift on a clip a slip rather than a move or copy", () => {
    expect(
      intentFor(press({ tool: "slip", hit: body, mod: true, shift: true }))
    ).toEqual({ kind: "slip", id: 7 })
  })

  it.each(TOOLS.filter((tool) => tool !== "playback"))(
    "selects with a box on Ctrl+drag over empty grid in the %s tool",
    (tool) => {
      expect(intentFor(press({ tool, mod: true }))).toEqual({
        kind: "marquee",
        additive: false,
      })
      expect(intentFor(press({ tool, mod: true, shift: true }))).toEqual({
        kind: "marquee",
        additive: true,
      })
    }
  )

  it.each<Tool>(["draw", "paint", "select"])(
    "keeps Ctrl on a clip a move in the %s tool, for the drop to copy",
    (tool) => {
      expect(intentFor(press({ tool, mod: true, hit: body }))).toEqual({
        kind: "move",
        id: 7,
        additive: false,
      })
      // Ctrl changes nothing about a press on an edge.
      expect(intentFor(press({ tool, mod: true, hit: endEdge }))).toEqual({
        kind: "resize-end",
        id: 7,
      })
    }
  )

  it.each<Tool>(["erase", "mute"])(
    "selects with Ctrl on a clip too in the %s tool, which does nothing else to it",
    (tool) => {
      expect(intentFor(press({ tool, mod: true, hit: body }))).toEqual({
        kind: "marquee",
        additive: false,
      })
    }
  )

  it.each<Tool>(["draw", "paint", "erase", "mute", "slip"])(
    "deletes with the right button in the %s tool",
    (tool) => {
      expect(intentFor(press({ tool, button: 2, hit: body }))).toEqual({
        kind: "erase",
      })
      expect(intentFor(press({ tool, button: 2 }))).toEqual({ kind: "erase" })
    }
  )

  it.each<Tool>(["draw", "paint", "erase", "mute", "slip"])(
    "opens the menu with the right button in the %s tool when set to",
    (tool) => {
      expect(
        intentFor(press({ tool, button: 2, hit: body, rightClickMenu: true }))
      ).toEqual({ kind: "menu", id: 7 })
      expect(
        intentFor(press({ tool, button: 2, rightClickMenu: true }))
      ).toEqual({ kind: "menu", id: null })
    }
  )

  it("opens a point's menu instead of deleting it when the right button is set to", () => {
    const point = { kind: "point", index: 2 } as const
    expect(
      intentFor(
        press({ button: 2, hit: body, inner: point, rightClickMenu: true })
      )
    ).toEqual({ kind: "point-menu", id: 7, index: 2 })
    // Playback seeks and has no menu either way.
    expect(
      intentFor(
        press({ tool: "playback", button: 2, hit: body, rightClickMenu: true })
      )
    ).toEqual({ kind: "none" })
  })

  it("opens a menu with the right button in the Select tool", () => {
    expect(intentFor(press({ tool: "select", button: 2, hit: body }))).toEqual({
      kind: "menu",
      id: 7,
    })
    expect(intentFor(press({ tool: "select", button: 2 }))).toEqual({
      kind: "menu",
      id: null,
    })
  })

  it.each(TOOLS)("pans with the middle button in the %s tool", (tool) => {
    expect(intentFor(press({ tool, button: 1, hit: body }))).toEqual({
      kind: "pan",
    })
  })

  it("ignores other buttons", () => {
    expect(intentFor(press({ button: 3 }))).toEqual({ kind: "none" })
  })

  it("scrubs empty grid, clips and handles even with modifiers", () => {
    for (const hit of [null, body, startEdge, endEdge]) {
      expect(
        intentFor(
          press({
            tool: "playback",
            hit,
            mod: true,
            shift: true,
            inner: { kind: "gain" },
          })
        )
      ).toEqual({ kind: "playback" })
    }
    expect(
      intentFor(press({ tool: "playback", button: 2, hit: body }))
    ).toEqual({ kind: "none" })
    expect(cursorFor("playback", "body", { kind: "gain" })).toBe("crosshair")
  })
})

describe("cursorFor", () => {
  it("shows what a press would do", () => {
    expect(cursorFor("draw", null)).toBe("crosshair")
    expect(cursorFor("draw", "body")).toBe("grab")
    expect(cursorFor("paint", "end-edge")).toBe("ew-resize")
    expect(cursorFor("select", null)).toBe("default")
    expect(cursorFor("select", "start-edge")).toBe("ew-resize")
    expect(cursorFor("erase", "body")).toBe("not-allowed")
    expect(cursorFor("mute", "body")).toBe("pointer")
    expect(cursorFor("mute", null)).toBe("default")
    expect(cursorFor("slip", null)).toBe("default")
    expect(cursorFor("slip", "body")).toBe("ew-resize")
    expect(cursorFor("slip", "start-edge")).toBe("ew-resize")
    expect(cursorFor("slip", "end-edge")).toBe("ew-resize")
  })
})
