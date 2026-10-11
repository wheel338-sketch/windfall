/*
 * A laid-out page as SVG: what the export dialog previews and what the
 * print flow sends to the printer. One element per drawing op, in order.
 */

import type { DrawOp, PathCommand, ScorePage } from "./draw"
import { FONTS } from "./fonts"

/** Three decimals of a point is finer than any printer. */
export function coordinate(value: number): string {
  const rounded = Math.round(value * 1000) / 1000
  return Object.is(rounded, -0) ? "0" : String(rounded)
}

function escapeXml(text: string): string {
  return text.replace(/[&<>"']/g, (character) => {
    switch (character) {
      case "&":
        return "&amp;"
      case "<":
        return "&lt;"
      case ">":
        return "&gt;"
      case '"':
        return "&quot;"
      default:
        return "&apos;"
    }
  })
}

/** The `d` attribute for a list of path commands. */
export function pathData(commands: readonly PathCommand[]): string {
  return commands
    .map((command) =>
      command[0] === "Z"
        ? "Z"
        : `${command[0]}${command.slice(1).map((value) => coordinate(value as number)).join(" ")}`
    )
    .join(" ")
}

/** One drawing op as an SVG element. */
export function opSvg(op: DrawOp): string {
  switch (op.type) {
    case "line":
      return `<line x1="${coordinate(op.x1)}" y1="${coordinate(op.y1)}" x2="${coordinate(op.x2)}" y2="${coordinate(op.y2)}" stroke="black" stroke-width="${coordinate(op.width)}"/>`
    case "path":
      return op.fill
        ? `<path d="${pathData(op.commands)}" fill="black"${op.evenOdd ? ' fill-rule="evenodd"' : ""}/>`
        : `<path d="${pathData(op.commands)}" fill="none" stroke="black" stroke-width="${coordinate(op.stroke ?? 1)}" stroke-linecap="round" stroke-linejoin="round"/>`
    case "ellipse":
      return `<ellipse cx="${coordinate(op.cx)}" cy="${coordinate(op.cy)}" rx="${coordinate(op.rx)}" ry="${coordinate(op.ry)}" fill="black"${
        op.rotation
          ? ` transform="rotate(${coordinate(op.rotation)} ${coordinate(op.cx)} ${coordinate(op.cy)})"`
          : ""
      }/>`
    case "text": {
      const font = FONTS[op.font]
      return `<text x="${coordinate(op.x)}" y="${coordinate(op.y)}" font-family="${escapeXml(font.css)}" font-size="${coordinate(op.size)}" font-weight="${font.weight}" font-style="${font.style}" text-anchor="${op.anchor}" fill="black">${escapeXml(op.text)}</text>`
    }
  }
}

/**
 * A whole page. Its size is in points, so it prints at its real size, and
 * the white sheet is part of the drawing so a dark theme previews paper.
 */
export function pageSvg(page: ScorePage): string {
  const width = coordinate(page.width)
  const height = coordinate(page.height)
  return [
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}pt" height="${height}pt" viewBox="0 0 ${width} ${height}">`,
    `<rect x="0" y="0" width="${width}" height="${height}" fill="white"/>`,
    ...page.ops.map(opSvg),
    "</svg>",
  ].join("\n")
}
