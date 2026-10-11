/*
 * A laid-out score as a PDF 1.4 file: vector pages, the four standard
 * fonts by name and nothing embedded. The text of the file is ASCII, so it
 * can be saved through the same UTF-8 writer the MusicXML export uses.
 *
 * A page's content flips the y axis once, so every op is written in the
 * layout's own coordinates: points from the top left, y down.
 */

import { ellipseCommands, textStart, type DrawOp, type PathCommand, type ScorePage } from "./draw"
import { FONTS, winAnsiCodes, type FontId } from "./fonts"

const FONT_IDS: readonly FontId[] = ["serif", "serif-bold", "serif-italic", "sans"]

/** A number as PDF writes it: no exponent, at most three decimals. */
export function pdfNumber(value: number): string {
  if (!Number.isFinite(value)) return "0"
  const rounded = Math.round(value * 1000) / 1000
  if (Object.is(rounded, -0) || rounded === 0) return "0"
  return rounded.toFixed(3).replace(/\.?0+$/, "")
}

/** A literal string in WinAnsi, with everything outside plain ASCII as octal. */
export function pdfString(text: string): string {
  let out = "("
  for (const code of winAnsiCodes(text)) {
    if (code === 40 || code === 41 || code === 92)
      out += `\\${String.fromCharCode(code)}`
    else if (code >= 32 && code <= 126) out += String.fromCharCode(code)
    else out += `\\${code.toString(8).padStart(3, "0")}`
  }
  return `${out})`
}

function pathOperators(commands: readonly PathCommand[]): string[] {
  return commands.map((command) => {
    switch (command[0]) {
      case "M":
        return `${pdfNumber(command[1])} ${pdfNumber(command[2])} m`
      case "L":
        return `${pdfNumber(command[1])} ${pdfNumber(command[2])} l`
      case "C":
        return `${command.slice(1).map((value) => pdfNumber(value as number)).join(" ")} c`
      default:
        return "h"
    }
  })
}

/** The content-stream operators that draw one op. */
export function opPdf(op: DrawOp): string[] {
  switch (op.type) {
    case "line":
      return [
        `${pdfNumber(op.width)} w`,
        `${pdfNumber(op.x1)} ${pdfNumber(op.y1)} m`,
        `${pdfNumber(op.x2)} ${pdfNumber(op.y2)} l`,
        "S",
      ]
    case "path":
      return op.fill
        ? [...pathOperators(op.commands), op.evenOdd ? "f*" : "f"]
        : [
            // Round ends and joins for the one stroke, then back to butt.
            "1 J 1 j",
            `${pdfNumber(op.stroke ?? 1)} w`,
            ...pathOperators(op.commands),
            "S",
            "0 J 0 j",
          ]
    case "ellipse":
      return [
        ...pathOperators(
          ellipseCommands(op.cx, op.cy, op.rx, op.ry, op.rotation)
        ),
        "f",
      ]
    case "text": {
      const font = FONT_IDS.indexOf(op.font) + 1
      // The page is flipped, so the text matrix flips each line back.
      return [
        "BT",
        `/F${font} ${pdfNumber(op.size)} Tf`,
        `1 0 0 -1 ${pdfNumber(textStart(op))} ${pdfNumber(op.y)} Tm`,
        `${pdfString(op.text)} Tj`,
        "ET",
      ]
    }
  }
}

/** The whole content stream of a page. */
export function pageContent(page: ScorePage): string {
  return [
    `1 0 0 -1 0 ${pdfNumber(page.height)} cm`,
    "0 g 0 G",
    ...page.ops.flatMap(opPdf),
  ].join("\n")
}

export type PdfInfo = { title?: string; author?: string }

/**
 * The file. Objects are numbered: 1 the catalog, 2 the page tree, 3 to 6 the
 * fonts, 7 the document information, then a page and its content for each
 * page of the layout.
 */
export function scorePdf(
  pages: readonly ScorePage[],
  info: PdfInfo = {}
): string {
  const objects: string[] = []
  const firstPage = 8
  const kids = pages.map((_, index) => `${firstPage + index * 2} 0 R`)
  objects.push("<< /Type /Catalog /Pages 2 0 R >>")
  objects.push(
    `<< /Type /Pages /Kids [${kids.join(" ")}] /Count ${pages.length} >>`
  )
  for (const id of FONT_IDS) {
    objects.push(
      `<< /Type /Font /Subtype /Type1 /BaseFont /${FONTS[id].pdf} /Encoding /WinAnsiEncoding >>`
    )
  }
  objects.push(
    `<< /Producer (Windfall)${info.title ? ` /Title ${pdfString(info.title)}` : ""}${info.author ? ` /Author ${pdfString(info.author)}` : ""} >>`
  )
  const fonts = FONT_IDS.map((_, index) => `/F${index + 1} ${3 + index} 0 R`)
  pages.forEach((page, index) => {
    const content = pageContent(page)
    objects.push(
      `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${pdfNumber(page.width)} ${pdfNumber(page.height)}] /Resources << /Font << ${fonts.join(" ")} >> >> /Contents ${firstPage + index * 2 + 1} 0 R >>`
    )
    objects.push(
      `<< /Length ${content.length} >>\nstream\n${content}\nendstream`
    )
  })

  let file = "%PDF-1.4\n"
  const offsets: number[] = []
  objects.forEach((body, index) => {
    offsets.push(file.length)
    file += `${index + 1} 0 obj\n${body}\nendobj\n`
  })
  const xref = file.length
  file += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`
  for (const offset of offsets) {
    file += `${String(offset).padStart(10, "0")} 00000 n \n`
  }
  file += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R /Info 7 0 R >>\nstartxref\n${xref}\n%%EOF\n`
  return file
}
