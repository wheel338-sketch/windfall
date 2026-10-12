/// <reference types="node" />
import { mkdirSync, writeFileSync } from "node:fs"
import { resolve } from "node:path"
import { expect, it } from "vitest"
import type { Note, Pattern } from "@/bindings"
import { PPQ } from "@/lib/units"
import { buildScore, type Score } from "./model"
import { layoutScore, STAFF_SPACE } from "./layout"
import { pageSvg } from "./svg"
import { scorePdf } from "./pdf"

function phrase(notes: Partial<Note>[], lengthSteps = 32): Pattern {
  return { id: 1, name: "Piano acceptance phrase", color: 0, lengthSteps,
    lanes: [{ channel: 7, notes: notes.map((note, index) => ({
      id: index + 1, start: 0, length: PPQ, key: 60, velocity: 1, pan: 0, ...note,
    })) }] }
}
const events = (score: Score) => score.measures.flatMap((measure) => measure.staves.flatMap((staff) => staff.voices.flatMap((voice) => voice.events)))

it("shows dotted rhythm, beat rests, barline ties and eighth triplets", () => {
  const score = buildScore(phrase([{ length: 720 }, { start: PPQ * 3, length: PPQ * 2, key: 67 }]), 7, { staff: "treble" })
  expect(events(score).some((event) => !event.rest && event.value === 8 && event.dots === 1)).toBe(true)
  expect(events(score).some((event) => event.rest && event.start === 720 && event.ticks === 240)).toBe(true)
  const tied = events(score).filter((event) => event.heads.some((head) => head.id === 2))
  expect(tied).toHaveLength(2)
  expect(tied[0].heads[0].tieStart).toBe(true)
  expect(tied[1].heads[0].tieStop).toBe(true)
  const triplet = buildScore(phrase([0, 320, 640].map((start) => ({ start, length: 320 })), 16), 7, { staff: "treble" })
  expect(triplet.measures[0].staves[0].voices[0].tuplets).toMatchObject([{ start: 0, end: 960, unit: 8 }])
  expect(events(triplet).filter((event) => !event.rest).map((event) => event.tuplet)).toEqual([0, 0, 0])
  const page = layoutScore(triplet).pages[0]
  // Music numerals are vector glyphs rather than text objects. The 3 has
  // two curved bowls, with four cubic segments and the triplet's small scale.
  const numeral = page.ops.filter((op) => op.type === "path" && !op.fill && op.stroke === 0.34 * (STAFF_SPACE * 0.7))
  expect(numeral).toHaveLength(1)
  expect(numeral[0].type === "path" && numeral[0].commands.map((command) => command[0])).toEqual(["M", "C", "C", "C", "C"])
})

it("cancels a key accidental, repeats it once per bar and restores a sharp", () => {
  const score = buildScore(phrase([66, 65, 65, 66, 65].map((key, index) => ({ key, start: index * PPQ }))), 7,
    { staff: "treble", key: { fifths: 1, mode: "major" } })
  expect(events(score).filter((event) => !event.rest).map((event) => event.heads[0].accidental))
    .toEqual([null, "natural", null, "sharp", "natural"])
})

it.each([{ numerator: 4, denominator: 4, count: 8, groups: [2, 2, 2, 2] },
  { numerator: 6, denominator: 8, count: 6, groups: [3, 3] }])("beams $numerator/$denominator in its beat groups", ({ numerator, denominator, count, groups }) => {
  const score = buildScore(phrase(Array.from({ length: count }, (_, index) => ({ start: index * 480, length: 480 })), count * 2), 7,
    { staff: "treble", baseSignature: { numerator, denominator } })
  const voice = score.measures[0].staves[0].voices[0]
  expect(voice.beams.map((beam) => beam.length)).toEqual(groups)
  expect(voice.beams.flat()).toEqual(Array.from({ length: count }, (_, index) => index))
  expect(layoutScore(score).pages[0].ops.some((op) => op.type === "path" && op.fill)).toBe(true)
})

it("keeps overlapping durations in opposite-stem voices and aligns their staff", () => {
  const score = buildScore(phrase([{ key: 72, length: PPQ * 4 },
    { key: 64, start: 480, length: 960 }, { key: 67, start: 1440, length: 960 }], 16), 7, { staff: "treble" })
  const voices = score.measures[0].staves[0].voices
  expect(voices).toHaveLength(2)
  expect(voices.map((voice) => voice.stems).sort()).toEqual(["down", "up"])
  expect(score.stats.simplified).toBe(0)
  // The off-beat quarter notes split at beat boundaries and retain tied heads.
  expect(voices.flatMap((voice) => voice.events.flatMap((event) => event.heads.map((head) => head.id))).sort()).toEqual([1, 2, 2, 3, 3])
  for (const id of [2, 3]) {
    const heads = events(score).flatMap(event => event.heads).filter(head => head.id === id)
    expect(heads.map(head => [head.tieStart, head.tieStop])).toEqual([[true, false], [false, true]])
  }
  const layout = layoutScore(score)
  expect(layout.systems).toHaveLength(1)
  expect(layout.pages).toHaveLength(1)
})

it("renders a known phrase to inspectable SVG/PDF bytes with matching pages, staff and noteheads", () => {
  const title = "Piano acceptance phrase"
  const score = buildScore(phrase([48, 52, 55, 60, 64, 67, 72, 76].map((key, index) => ({ key, start: index * PPQ }))), 7,
    { staff: "grand", key: { fifths: 0, mode: "major" } })
  const layout = layoutScore(score, { title, tempoBpm: 120 })
  expect(layout.pages).toHaveLength(1)
  const svg = pageSvg(layout.pages[0])
  const document = new DOMParser().parseFromString(svg, "image/svg+xml")
  expect(document.querySelector("parsererror")).toBeNull()
  expect(document.documentElement.textContent).toContain(title)
  expect(document.documentElement.textContent).toContain("= 120")
  expect([...document.querySelectorAll("ellipse")].filter((head) =>
    Math.abs(Number(head.getAttribute("rx")) - 0.6 * STAFF_SPACE) < 1e-6 &&
    Math.abs(Number(head.getAttribute("ry")) - 0.39 * STAFF_SPACE) < 1e-6)).toHaveLength(8)
  const staffLines = layout.pages[0].ops.filter((op) => op.type === "line" && op.width === 0.5 && op.y1 === op.y2 &&
    layout.systems.some((system) => op.x1 === system.left + 2 * STAFF_SPACE && op.x2 === system.right))
  expect(staffLines).toHaveLength(layout.systems.length * 10)
  const file = scorePdf(layout.pages, { title })
  const bytes = new TextEncoder().encode(file)
  expect(bytes.length).toBe(file.length) // all offsets are byte offsets, not UTF-16
  const xref = Number(/startxref\n(\d+)\n%%EOF\n$/.exec(file)?.[1])
  expect(file.slice(xref, xref + 5)).toBe("xref\n")
  const rows = file.slice(xref).split("\n")
  const count = Number(rows[1].split(" ")[1])
  for (let id = 1; id < count; id++) {
    const offset = Number(rows[id + 2].slice(0, 10))
    expect(file.slice(offset, offset + `${id} 0 obj\n`.length)).toBe(`${id} 0 obj\n`)
  }
  expect(file).toContain("/Count 1")
  expect(file).toContain(`/Title (${title})`)
  expect(file).toContain("(= 120) Tj")
  expect(file.match(/\/Type \/Page\b/g)).toHaveLength(1)
  // Parse the actual, uncompressed PDF content independently of opPdf. Four
  // cubic arcs close each filled ellipse; the opposite endpoints recover its
  // radii even when a notehead is rotated. Tempo and clef dots have other sizes.
  const number = "(-?\\d+(?:\\.\\d+)?)"
  const pair = `${number} ${number}`
  const cubic = `${pair} ${pair} ${pair} c\\n`
  const ellipses = [...file.matchAll(new RegExp(`${pair} m\\n${cubic.repeat(4)}h\\nf\\n`, "g"))]
  const musicHeads = ellipses.filter((match) => {
    const x = (Number(match[1]) + Number(match[13])) / 2
    const y = (Number(match[2]) + Number(match[14])) / 2
    const rx = Math.hypot(Number(match[1]) - x, Number(match[2]) - y)
    const ry = Math.hypot(Number(match[7]) - x, Number(match[8]) - y)
    return Math.abs(rx - 0.6 * STAFF_SPACE) < .003 && Math.abs(ry - 0.39 * STAFF_SPACE) < .003
  })
  expect(musicHeads).toHaveLength(8)
  const pdfLines = [...file.matchAll(new RegExp(`0\\.5 w\\n${pair} m\\n${pair} l\\nS\\n`, "g"))]
  expect(pdfLines.filter((match) => Number(match[2]) === Number(match[4]) && Number(match[3]) - Number(match[1]) > 100))
    .toHaveLength(layout.systems.length * 10)
  const streams = [...file.matchAll(/<< \/Length (\d+) >>\nstream\n([\s\S]*?)\nendstream/g)]
  expect(streams).toHaveLength(1)
  expect(new TextEncoder().encode(streams[0][2]).length).toBe(Number(streams[0][1]))
  // The integrator can open these exact asserted exports in a reader.
  const folder = resolve(process.cwd(), "../../docs/qa/p2-piano-artifacts")
  mkdirSync(folder, { recursive: true })
  writeFileSync(resolve(folder, "known-phrase.svg"), svg)
  writeFileSync(resolve(folder, "known-phrase.pdf"), bytes)
})
