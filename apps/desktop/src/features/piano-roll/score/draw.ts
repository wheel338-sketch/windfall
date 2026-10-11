/*
 * The drawing primitives a laid-out page is made of. Coordinates are points
 * (1/72 inch) from the top left of the page, y down. Everything is black.
 * The SVG and PDF renderers read the same list.
 */

import { textWidth, type FontId } from "./fonts"

export type PathCommand =
  | readonly ["M", number, number]
  | readonly ["L", number, number]
  | readonly ["C", number, number, number, number, number, number]
  | readonly ["Z"]

export type DrawOp =
  | {
      type: "line"
      x1: number
      y1: number
      x2: number
      y2: number
      width: number
    }
  | {
      type: "path"
      commands: PathCommand[]
      /** Filled, or stroked with round ends at this width. */
      fill: boolean
      evenOdd?: boolean
      stroke?: number
    }
  | {
      /** A filled ellipse, turned clockwise by `rotation` degrees. */
      type: "ellipse"
      cx: number
      cy: number
      rx: number
      ry: number
      rotation: number
    }
  | {
      type: "text"
      x: number
      /** The baseline. */
      y: number
      text: string
      size: number
      font: FontId
      anchor: "start" | "middle" | "end"
    }

export type ScorePage = { width: number; height: number; ops: DrawOp[] }
export type Bounds = { x0: number; y0: number; x1: number; y1: number }

export function translateOp(op: DrawOp, dx: number, dy: number): DrawOp {
  switch (op.type) {
    case "line":
      return {
        ...op,
        x1: op.x1 + dx,
        y1: op.y1 + dy,
        x2: op.x2 + dx,
        y2: op.y2 + dy,
      }
    case "path":
      return {
        ...op,
        commands: op.commands.map((command): PathCommand => {
          if (command[0] === "Z") return command
          if (command[0] === "C")
            return [
              "C",
              command[1] + dx,
              command[2] + dy,
              command[3] + dx,
              command[4] + dy,
              command[5] + dx,
              command[6] + dy,
            ]
          return [command[0], command[1] + dx, command[2] + dy]
        }),
      }
    case "ellipse":
      return { ...op, cx: op.cx + dx, cy: op.cy + dy }
    case "text":
      return { ...op, x: op.x + dx, y: op.y + dy }
  }
}

/** Where text starts, given its anchor and measured width. */
export function textStart(op: Extract<DrawOp, { type: "text" }>): number {
  const width = textWidth(op.text, op.font, op.size)
  return op.anchor === "start"
    ? op.x
    : op.anchor === "middle"
      ? op.x - width / 2
      : op.x - width
}

/**
 * A box that contains the op. Curves are bounded by their control points and
 * text by the font tables, so the box can be a little larger than the ink.
 */
export function opBounds(op: DrawOp): Bounds {
  switch (op.type) {
    case "line": {
      const half = op.width / 2
      return {
        x0: Math.min(op.x1, op.x2) - half,
        y0: Math.min(op.y1, op.y2) - half,
        x1: Math.max(op.x1, op.x2) + half,
        y1: Math.max(op.y1, op.y2) + half,
      }
    }
    case "path": {
      const bounds = { x0: Infinity, y0: Infinity, x1: -Infinity, y1: -Infinity }
      for (const command of op.commands) {
        for (let index = 1; index + 1 < command.length; index += 2) {
          const x = command[index] as number
          const y = command[index + 1] as number
          bounds.x0 = Math.min(bounds.x0, x)
          bounds.y0 = Math.min(bounds.y0, y)
          bounds.x1 = Math.max(bounds.x1, x)
          bounds.y1 = Math.max(bounds.y1, y)
        }
      }
      const half = (op.stroke ?? 0) / 2
      return {
        x0: bounds.x0 - half,
        y0: bounds.y0 - half,
        x1: bounds.x1 + half,
        y1: bounds.y1 + half,
      }
    }
    case "ellipse": {
      const reach = Math.max(op.rx, op.ry)
      return {
        x0: op.cx - reach,
        y0: op.cy - reach,
        x1: op.cx + reach,
        y1: op.cy + reach,
      }
    }
    case "text": {
      const x0 = textStart(op)
      return {
        x0,
        y0: op.y - op.size * 0.9,
        x1: x0 + textWidth(op.text, op.font, op.size),
        y1: op.y + op.size * 0.25,
      }
    }
  }
}

export function boundsOf(ops: readonly DrawOp[]): Bounds {
  const bounds = { x0: Infinity, y0: Infinity, x1: -Infinity, y1: -Infinity }
  for (const op of ops) {
    const one = opBounds(op)
    bounds.x0 = Math.min(bounds.x0, one.x0)
    bounds.y0 = Math.min(bounds.y0, one.y0)
    bounds.x1 = Math.max(bounds.x1, one.x1)
    bounds.y1 = Math.max(bounds.y1, one.y1)
  }
  return bounds
}

/** Four Bezier arcs for an ellipse turned clockwise by `rotation` degrees. */
export function ellipseCommands(
  cx: number,
  cy: number,
  rx: number,
  ry: number,
  rotation: number
): PathCommand[] {
  const angle = (rotation * Math.PI) / 180
  const cos = Math.cos(angle)
  const sin = Math.sin(angle)
  const at = (x: number, y: number): [number, number] => [
    cx + x * cos - y * sin,
    cy + x * sin + y * cos,
  ]
  // The control distance that makes a cubic follow a quarter circle.
  const k = 0.5522847498
  const arc = (
    c1: [number, number],
    c2: [number, number],
    to: [number, number]
  ): PathCommand => ["C", ...at(...c1), ...at(...c2), ...at(...to)]
  return [
    ["M", ...at(rx, 0)],
    arc([rx, ry * k], [rx * k, ry], [0, ry]),
    arc([-rx * k, ry], [-rx, ry * k], [-rx, 0]),
    arc([-rx, -ry * k], [-rx * k, -ry], [0, -ry]),
    arc([rx * k, -ry], [rx, -ry * k], [rx, 0]),
    ["Z"],
  ]
}
