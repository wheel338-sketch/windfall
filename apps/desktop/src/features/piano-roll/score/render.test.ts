import { describe, expect, it } from "vitest"

import type { Note, Pattern } from "@/bindings"
import { PPQ } from "@/lib/units"

import { opBounds, type DrawOp, type ScorePage } from "./draw"
import { PAPERS, layoutScore } from "./layout"
import { buildScore } from "./model"
import { pageContent, pdfNumber, pdfString, scorePdf } from "./pdf"
import { opSvg, pageSvg, pathData } from "./svg"

const CHANNEL = 7

function pattern(notes: Partial<Note>[], lengthSteps = 16): Pattern {
  return {
    id: 1,
    name: "Verse & <Chorus>",
    color: 0,
    lengthSteps,
    lanes: notes.length
      ? [
          {
            channel: CHANNEL,
            notes: notes.map((note, index) => ({
              id: index + 1,
              start: 0,
              length: PPQ,
              key: 60,
              velocity: 0.8,
              pan: 0,
              ...note,
            })),
          },
        ]
      : [],
  } as Pattern
}

/** A page with one op of every kind. */
const SAMPLE: ScorePage = {
  width: 200,
  height: 100,
  ops: [
    { type: "line", x1: 10, y1: 20, x2: 190, y2: 20, width: 0.5 },
    {
      type: "path",
      fill: true,
      evenOdd: true,
      commands: [["M", 10, 30], ["L", 20, 30], ["C", 20, 35, 15, 40, 10, 40], ["Z"]],
    },
    {
      type: "path",
      fill: false,
      stroke: 1.25,
      commands: [["M", 30, 30], ["L", 40, 40]],
    },
    { type: "ellipse", cx: 60, cy: 50, rx: 3, ry: 2, rotation: -20 },
    {
      type: "text",
      x: 100,
      y: 90,
      text: "Tom & Jerry (é) \\ <1>",
      size: 10,
      font: "serif-bold",
      anchor: "middle",
    },
  ],
}

/** The objects of a PDF by number, read through its cross-reference table. */
function parsePdf(file: string) {
  const startxref = Number(/startxref\n(\d+)\n%%EOF\n$/.exec(file)?.[1])
  const table = file.slice(startxref)
  const count = Number(/^xref\n0 (\d+)\n/.exec(table)?.[1])
  const rows = table.split("\n").slice(2, 2 + count)
  const objects = new Map<number, string>()
  rows.forEach((row, index) => {
    if (index === 0) return
    const offset = Number(row.slice(0, 10))
    const header = `${index} 0 obj\n`
    expect(file.slice(offset, offset + header.length)).toBe(header)
    const end = file.indexOf("\nendobj\n", offset)
    objects.set(index, file.slice(offset + header.length, end))
  })
  return { startxref, count, rows, objects }
}

describe("score SVG", () => {
  it("writes one element for each op of a page, well formed", () => {
    const svg = pageSvg(SAMPLE)
    const parsed = new DOMParser().parseFromString(svg, "image/svg+xml")
    expect(parsed.querySelector("parsererror")).toBeNull()
    const root = parsed.documentElement
    expect(root.tagName).toBe("svg")
    expect(root.getAttribute("viewBox")).toBe("0 0 200 100")
    expect(root.getAttribute("width")).toBe("200pt")
    expect(root.querySelectorAll("line")).toHaveLength(1)
    expect(root.querySelectorAll("path")).toHaveLength(2)
    expect(root.querySelectorAll("ellipse")).toHaveLength(1)
    expect(root.querySelectorAll("text")).toHaveLength(1)
    // The sheet itself, so a dark theme previews paper.
    expect(root.querySelectorAll("rect")).toHaveLength(1)
    expect(root.querySelector("text")?.textContent).toBe(
      "Tom & Jerry (é) \\ <1>"
    )
  })

  it("fills, strokes and turns as the op says", () => {
    expect(opSvg(SAMPLE.ops[1])).toContain('fill-rule="evenodd"')
    const stroked = opSvg(SAMPLE.ops[2])
    expect(stroked).toContain('fill="none"')
    expect(stroked).toContain('stroke-width="1.25"')
    expect(stroked).toContain('stroke-linecap="round"')
    expect(opSvg(SAMPLE.ops[3])).toContain('transform="rotate(-20 60 50)"')
    const text = opSvg(SAMPLE.ops[4])
    expect(text).toContain('text-anchor="middle"')
    expect(text).toContain('font-weight="bold"')
    expect(text).toContain("Tom &amp; Jerry")
    expect(text).toContain("&lt;1&gt;")
    expect(pathData([["M", 1, 2], ["L", 3.14159, 4], ["Z"]])).toBe(
      "M1 2 L3.142 4 Z"
    )
  })
})

describe("score PDF", () => {
  it("is an ASCII file whose cross-reference table finds every object", () => {
    const file = scorePdf([SAMPLE, SAMPLE], { title: "Two (pages)" })
    expect(file.startsWith("%PDF-1.4\n")).toBe(true)
    expect(file.endsWith("%%EOF\n")).toBe(true)
    // Nothing outside printable ASCII and newlines: it survives a text writer.
    expect(/^[\n\x20-\x7e]*$/.test(file)).toBe(true)

    const { count, rows, objects } = parsePdf(file)
    // Catalog, page tree, four fonts, information, then two objects per page.
    expect(count).toBe(1 + 7 + 2 * 2)
    expect(rows[0]).toBe("0000000000 65535 f ")
    expect(rows.every((row) => row.length === 19)).toBe(true)
    expect(objects.get(1)).toContain("/Type /Catalog")
    expect(objects.get(2)).toContain("/Count 2")
    expect(objects.get(2)).toContain("/Kids [8 0 R 10 0 R]")
    expect(objects.get(3)).toContain("/BaseFont /Times-Roman")
    expect(objects.get(6)).toContain("/BaseFont /Helvetica")
    expect(objects.get(7)).toContain("/Title (Two \\(pages\\))")
    for (const page of [8, 10]) {
      const body = objects.get(page) ?? ""
      expect(body).toContain("/Type /Page")
      expect(body).toContain("/MediaBox [0 0 200 100]")
      expect(body).toContain(`/Contents ${page + 1} 0 R`)
      const stream = objects.get(page + 1) ?? ""
      const length = Number(/\/Length (\d+)/.exec(stream)?.[1])
      const content = stream.slice(
        stream.indexOf("stream\n") + "stream\n".length,
        stream.lastIndexOf("\nendstream")
      )
      expect(content.length).toBe(length)
      expect(content).toBe(pageContent(SAMPLE))
    }
    expect(file).toContain("/Root 1 0 R")
  })

  it("draws every op with the operators a reader expects", () => {
    const content = pageContent(SAMPLE)
    const lines = content.split("\n")
    // The page is flipped once, so ops keep the layout's coordinates.
    expect(lines[0]).toBe("1 0 0 -1 0 100 cm")
    expect(content).toContain("0.5 w\n10 20 m\n190 20 l\nS")
    expect(content).toContain("10 30 m\n20 30 l\n20 35 15 40 10 40 c\nh\nf*")
    expect(content).toContain("1 J 1 j\n1.25 w\n30 30 m\n40 40 l\nS\n0 J 0 j")
    // The ellipse is four curves, filled.
    const curves = lines.filter((line) => line.endsWith(" c"))
    expect(curves).toHaveLength(1 + 4)
    // Text is un-flipped by its own matrix, in the bold serif face.
    expect(content).toContain("/F2 10 Tf")
    expect(content).toMatch(/1 0 0 -1 [\d.]+ 90 Tm/)
    expect(content).toContain("(Tom & Jerry \\(\\351\\) \\\\ <1>) Tj")
  })

  it("writes plain numbers and escaped strings", () => {
    expect(pdfNumber(0)).toBe("0")
    expect(pdfNumber(-0)).toBe("0")
    expect(pdfNumber(1.5)).toBe("1.5")
    expect(pdfNumber(1 / 3)).toBe("0.333")
    expect(pdfNumber(1e-7)).toBe("0")
    expect(pdfNumber(Number.NaN)).toBe("0")
    expect(pdfNumber(595.28)).toBe("595.28")
    expect(pdfString("a(b)c\\")).toBe("(a\\(b\\)c\\\\)")
    expect(pdfString("—")).toBe("(\\227)")
    expect(pdfString("中")).toBe("(?)")
  })
})

describe("engraved pages", () => {
  const inside = (page: ScorePage, op: DrawOp) => {
    const bounds = opBounds(op)
    return (
      bounds.x0 >= -0.5 &&
      bounds.y0 >= -0.5 &&
      bounds.x1 <= page.width + 0.5 &&
      bounds.y1 <= page.height + 0.5
    )
  }

  it("an empty channel is one page of rests", () => {
    const score = buildScore(pattern([]), CHANNEL)
    expect(score.stats.notes).toBe(0)
    expect(score.measures.length).toBeGreaterThanOrEqual(1)
    const layout = layoutScore(score, { paper: "a4", title: "Empty" })
    expect(layout.pages).toHaveLength(1)
    expect(layout.pages[0].width).toBe(PAPERS.a4.width)
    expect(layout.pages[0].ops.length).toBeGreaterThan(10)
    const file = scorePdf(layout.pages)
    expect(parsePdf(file).objects.get(2)).toContain("/Count 1")
  })

  it("a long pattern takes several pages and shows every measure once", () => {
    // 96 bars of 4/4 with a busy bar of sixteenths in each.
    const bars = 96
    const notes: Partial<Note>[] = []
    for (let bar = 0; bar < bars; bar += 1) {
      for (let step = 0; step < 16; step += 1) {
        notes.push({
          start: bar * PPQ * 4 + step * (PPQ / 4),
          length: PPQ / 4,
          key: 55 + ((bar + step * 3) % 14),
        })
      }
    }
    const score = buildScore(pattern(notes, bars * 16), CHANNEL, {
      smallest: 16,
    })
    expect(score.stats.notes).toBe(notes.length)
    expect(score.stats.quantised).toBe(0)
    expect(score.measures).toHaveLength(bars)
    for (const paper of ["a4", "letter"] as const) {
      const layout = layoutScore(score, { paper, title: "Long", tempoBpm: 128 })
      expect(layout.pages.length).toBeGreaterThan(1)
      const shown = layout.systems.flatMap((system) => system.measures)
      expect(shown).toEqual(Array.from({ length: bars }, (_, index) => index))
      expect(new Set(layout.systems.map((system) => system.page)).size).toBe(
        layout.pages.length
      )
      for (const page of layout.pages) {
        expect(page.width).toBe(PAPERS[paper].width)
        expect(page.height).toBe(PAPERS[paper].height)
        for (const op of page.ops) expect(inside(page, op)).toBe(true)
      }
      for (const system of layout.systems) {
        expect(system.left).toBeGreaterThanOrEqual(0)
        expect(system.right).toBeLessThanOrEqual(PAPERS[paper].width)
        expect(system.bottom).toBeLessThanOrEqual(PAPERS[paper].height)
      }
      // The same pages render to SVG and PDF alike.
      const file = scorePdf(layout.pages)
      expect(parsePdf(file).objects.get(2)).toContain(
        `/Count ${layout.pages.length}`
      )
      const svg = pageSvg(layout.pages[0])
      const parsed = new DOMParser().parseFromString(svg, "image/svg+xml")
      expect(parsed.querySelector("parsererror")).toBeNull()
    }
  })

  it("counts notes it had to move and reads the key from the notes", () => {
    const off = buildScore(
      pattern([{ start: 7, length: PPQ - 13, key: 62 }]),
      CHANNEL,
      { smallest: 16 }
    )
    expect(off.stats.quantised).toBe(1)

    // A D major scale has two sharps.
    const scale = [62, 64, 66, 67, 69, 71, 73, 74].map((key, index) => ({
      key,
      start: index * PPQ,
      length: PPQ,
    }))
    const detected = buildScore(pattern(scale, 32), CHANNEL, { key: "auto" })
    expect(detected.keyDetected).toBe(true)
    expect(detected.key.fifths).toBe(2)
    const chosen = buildScore(pattern(scale, 32), CHANNEL, {
      key: { fifths: -3, mode: "major" },
    })
    expect(chosen.keyDetected).toBe(false)
    expect(chosen.key.fifths).toBe(-3)
    // One staff when asked for one.
    expect(buildScore(pattern(scale, 32), CHANNEL, { staff: "treble" }).staves).toEqual(["treble"])
    expect(buildScore(pattern(scale, 32), CHANNEL, { staff: "grand" }).staves).toEqual(["treble", "bass"])
  })
})
