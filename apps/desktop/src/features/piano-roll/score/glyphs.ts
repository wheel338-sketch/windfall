/*
 * Music symbols as plain vector shapes, drawn for Windfall. No music font is
 * used and no outline is taken from one: every symbol here is a handful of
 * ellipses, straight strokes and hand-placed curves. Shapes are written in
 * staff spaces and placed at a point on the page.
 */

import type { NoteValue } from "./model"
import { ellipseCommands, type DrawOp, type PathCommand } from "./draw"

/** Half the width of a stemmed notehead, in staff spaces. */
export const HEAD_HALF = 0.59
export const WHOLE_HEAD_HALF = 0.78
export const STEM_WIDTH = 0.13
export const BEAM_THICKNESS = 0.5
export const BEAM_GAP = 0.75

type Spec = readonly PathCommand[]

function place(
  spec: Spec,
  x: number,
  y: number,
  scaleX: number,
  scaleY = scaleX
): PathCommand[] {
  return spec.map((command): PathCommand => {
    if (command[0] === "Z") return command
    if (command[0] === "C")
      return [
        "C",
        x + command[1] * scaleX,
        y + command[2] * scaleY,
        x + command[3] * scaleX,
        y + command[4] * scaleY,
        x + command[5] * scaleX,
        y + command[6] * scaleY,
      ]
    return [command[0], x + command[1] * scaleX, y + command[2] * scaleY]
  })
}

function filled(commands: PathCommand[], evenOdd = false): DrawOp {
  return { type: "path", commands, fill: true, ...(evenOdd && { evenOdd }) }
}

function stroked(commands: PathCommand[], width: number): DrawOp {
  return { type: "path", commands, fill: false, stroke: width }
}

function polygon(points: readonly (readonly [number, number])[]): DrawOp {
  return filled([
    ...points.map(
      ([x, y], index): PathCommand => [index ? "L" : "M", x, y]
    ),
    ["Z"],
  ])
}

function disc(cx: number, cy: number, radius: number): DrawOp {
  return { type: "ellipse", cx, cy, rx: radius, ry: radius, rotation: 0 }
}

/** A notehead centred on a point: filled, open (half) or wide open (whole). */
export function notehead(
  cx: number,
  cy: number,
  sp: number,
  value: NoteValue
): DrawOp {
  if (value >= 4)
    return {
      type: "ellipse",
      cx,
      cy,
      rx: 0.6 * sp,
      ry: 0.39 * sp,
      rotation: -20,
    }
  if (value === 2)
    return filled(
      [
        ...ellipseCommands(cx, cy, 0.6 * sp, 0.39 * sp, -20),
        ...ellipseCommands(cx, cy, 0.47 * sp, 0.19 * sp, -30),
      ],
      true
    )
  return filled(
    [
      ...ellipseCommands(cx, cy, WHOLE_HEAD_HALF * sp, 0.42 * sp, 0),
      ...ellipseCommands(cx, cy, 0.36 * sp, 0.27 * sp, 35),
    ],
    true
  )
}

export function dot(cx: number, cy: number, sp: number): DrawOp {
  return disc(cx, cy, 0.2 * sp)
}

const FLAG: Spec = [
  ["M", 0, 0],
  ["C", 0.05, 0.75, 1.2, 1.25, 0.9, 2.75],
  ["C", 1.0, 1.9, 0.55, 1.45, 0, 1.15],
  ["Z"],
]

/** Flags at a stem tip, one per halving below a quarter note. */
export function flags(
  x: number,
  tip: number,
  sp: number,
  up: boolean,
  count: number
): DrawOp[] {
  const direction = up ? 1 : -1
  return Array.from({ length: count }, (_, index) =>
    filled(
      place(FLAG, x, tip + direction * index * 0.8 * sp, sp, direction * sp)
    )
  )
}

/** A whole rest hangs from its line; a half rest sits on its line. */
export function blockRest(
  cx: number,
  line: number,
  sp: number,
  hanging: boolean
): DrawOp {
  const top = hanging ? line : line - 0.5 * sp
  return polygon([
    [cx - 0.62 * sp, top],
    [cx + 0.62 * sp, top],
    [cx + 0.62 * sp, top + 0.5 * sp],
    [cx - 0.62 * sp, top + 0.5 * sp],
  ])
}

/** A quarter rest centred on a point: a thin zigzag with a heavy middle. */
export function quarterRest(cx: number, cy: number, sp: number): DrawOp[] {
  return [
    stroked(
      place(
        [
          ["M", -0.22, -1.5],
          ["L", 0.34, -0.72],
          ["L", -0.2, -0.02],
          ["L", 0.36, 0.62],
        ],
        cx,
        cy,
        sp
      ),
      0.16 * sp
    ),
    stroked(
      place(
        [
          ["M", 0.24, -0.6],
          ["L", -0.1, -0.14],
        ],
        cx,
        cy,
        sp
      ),
      0.4 * sp
    ),
    stroked(
      place(
        [
          ["M", 0.36, 0.62],
          ["C", -0.3, 0.25, -0.62, 0.85, -0.02, 1.4],
        ],
        cx,
        cy,
        sp
      ),
      0.24 * sp
    ),
  ]
}

/** Eighth, sixteenth and 1/32 rests: a slanted stroke with one to three hooks. */
export function hookRest(
  cx: number,
  cy: number,
  sp: number,
  count: number
): DrawOp[] {
  const hooks = count === 1 ? [-0.5] : count === 2 ? [-0.5, 0.5] : [-1.5, -0.5, 0.5]
  const top = hooks[0] - 0.35
  const bottom = hooks[hooks.length - 1] + 1.5
  const along = (y: number) => cx + (0.1 - y * 0.27) * sp
  const ops: DrawOp[] = [
    {
      type: "line",
      x1: along(top),
      y1: cy + top * sp,
      x2: along(bottom),
      y2: cy + bottom * sp,
      width: 0.15 * sp,
    },
  ]
  for (const hook of hooks) {
    const x = along(hook - 0.3)
    const y = cy + hook * sp
    ops.push(
      disc(x - 0.85 * sp, y - 0.05 * sp, 0.24 * sp),
      stroked(
        [
          ["M", x - 0.85 * sp, y + 0.1 * sp],
          [
            "C",
            x - 0.6 * sp,
            y + 0.4 * sp,
            x - 0.2 * sp,
            y + 0.2 * sp,
            x,
            y - 0.3 * sp,
          ],
        ],
        0.13 * sp
      )
    )
  }
  return ops
}

function slantedBar(
  cx: number,
  cy: number,
  sp: number,
  half: number,
  rise: number
): DrawOp {
  return polygon([
    [cx - half * sp, cy + (rise - 0.15) * sp],
    [cx + half * sp, cy - (rise + 0.15) * sp],
    [cx + half * sp, cy - (rise - 0.15) * sp],
    [cx - half * sp, cy + (rise + 0.15) * sp],
  ])
}

function upright(
  x: number,
  cy: number,
  sp: number,
  from: number,
  to: number
): DrawOp {
  return {
    type: "line",
    x1: x,
    y1: cy + from * sp,
    x2: x,
    y2: cy + to * sp,
    width: 0.11 * sp,
  }
}

/** Half the width an accidental needs, in staff spaces. */
export const ACCIDENTAL_HALF = 0.55

export function sharp(cx: number, cy: number, sp: number): DrawOp[] {
  return [
    upright(cx - 0.2 * sp, cy, sp, -1.3, 1.4),
    upright(cx + 0.2 * sp, cy, sp, -1.4, 1.3),
    slantedBar(cx, cy - 0.42 * sp, sp, 0.5, 0.13),
    slantedBar(cx, cy + 0.42 * sp, sp, 0.5, 0.13),
  ]
}

export function natural(cx: number, cy: number, sp: number): DrawOp[] {
  return [
    upright(cx - 0.24 * sp, cy, sp, -1.45, 0.62),
    upright(cx + 0.24 * sp, cy, sp, -0.62, 1.45),
    slantedBar(cx, cy - 0.45 * sp, sp, 0.24, 0.1),
    slantedBar(cx, cy + 0.45 * sp, sp, 0.24, 0.1),
  ]
}

export function flat(cx: number, cy: number, sp: number): DrawOp[] {
  return [
    upright(cx - 0.3 * sp, cy, sp, -1.8, 0.62),
    filled(
      place(
        [
          ["M", -0.3, -0.3],
          ["C", 0.0, -0.75, 0.7, -0.6, 0.52, -0.05],
          ["C", 0.4, 0.3, 0.0, 0.45, -0.3, 0.66],
          ["L", -0.3, 0.38],
          ["C", -0.05, 0.22, 0.22, 0.05, 0.26, -0.18],
          ["C", 0.3, -0.42, -0.05, -0.42, -0.3, -0.08],
          ["Z"],
        ],
        cx,
        cy,
        sp
      )
    ),
  ]
}

export function accidental(
  kind: "sharp" | "flat" | "natural",
  cx: number,
  cy: number,
  sp: number
): DrawOp[] {
  return kind === "sharp"
    ? sharp(cx, cy, sp)
    : kind === "flat"
      ? flat(cx, cy, sp)
      : natural(cx, cy, sp)
}

export const CLEF_WIDTH = 2.9

/** A G clef. `top` is the top staff line; the curl circles the G line. */
export function trebleClef(x: number, top: number, sp: number): DrawOp[] {
  return [
    stroked(
      place(
        [
          ["M", 1.32, 3.38],
          ["C", 0.92, 3.42, 0.78, 2.8, 1.22, 2.6],
          ["C", 1.72, 2.4, 2.28, 2.9, 2.12, 3.48],
          ["C", 1.96, 4.02, 1.1, 4.22, 0.58, 3.8],
          ["C", 0.02, 3.34, 0.16, 2.48, 0.82, 1.8],
          ["C", 1.38, 1.22, 2.02, 0.42, 1.92, -0.62],
          ["C", 1.86, -1.36, 1.3, -1.52, 1.16, -0.78],
          ["C", 1.06, -0.24, 1.28, 0.9, 1.42, 1.72],
          ["L", 1.86, 4.72],
          ["C", 1.98, 5.42, 1.3, 5.72, 0.88, 5.32],
        ],
        x,
        top,
        sp
      ),
      0.2 * sp
    ),
    disc(x + 1.02 * sp, top + 5.18 * sp, 0.27 * sp),
  ]
}

/** An F clef. Its head and its two dots mark the F line, one below the top. */
export function bassClef(x: number, top: number, sp: number): DrawOp[] {
  return [
    disc(x + 0.5 * sp, top + 1.1 * sp, 0.33 * sp),
    filled(
      place(
        [
          ["M", 0.18, 1.1],
          ["C", 0.12, 0.2, 0.95, -0.12, 1.55, 0.02],
          ["C", 2.45, 0.22, 2.62, 1.35, 2.05, 2.25],
          ["C", 1.62, 2.95, 0.9, 3.42, 0.1, 3.82],
          ["C", 0.85, 3.3, 1.4, 2.78, 1.68, 2.05],
          ["C", 1.98, 1.28, 1.85, 0.42, 1.42, 0.26],
          ["C", 1.0, 0.1, 0.48, 0.38, 0.44, 1.0],
          ["Z"],
        ],
        x,
        top,
        sp
      )
    ),
    disc(x + 2.75 * sp, top + 0.5 * sp, 0.2 * sp),
    disc(x + 2.75 * sp, top + 1.5 * sp, 0.2 * sp),
  ]
}

const DIGITS: readonly Spec[] = [
  [
    ["M", 0.65, 0.2],
    ["C", 0.2, 0.2, 0.2, 0.75, 0.2, 1],
    ["C", 0.2, 1.25, 0.2, 1.8, 0.65, 1.8],
    ["C", 1.1, 1.8, 1.1, 1.25, 1.1, 1],
    ["C", 1.1, 0.75, 1.1, 0.2, 0.65, 0.2],
    ["Z"],
  ],
  [
    ["M", 0.3, 0.65],
    ["L", 0.72, 0.2],
    ["L", 0.72, 1.8],
    ["M", 0.35, 1.8],
    ["L", 1.08, 1.8],
  ],
  [
    ["M", 0.22, 0.62],
    ["C", 0.25, 0.08, 1.12, 0.05, 1.08, 0.66],
    ["C", 1.05, 1.1, 0.45, 1.25, 0.2, 1.8],
    ["L", 1.1, 1.8],
  ],
  [
    ["M", 0.22, 0.48],
    ["C", 0.38, 0.08, 1.1, 0.1, 1.05, 0.6],
    ["C", 1.02, 0.9, 0.8, 1, 0.55, 1],
    ["C", 0.85, 1, 1.1, 1.1, 1.1, 1.4],
    ["C", 1.1, 1.92, 0.35, 1.95, 0.2, 1.52],
  ],
  [
    ["M", 0.85, 1.8],
    ["L", 0.85, 0.2],
    ["L", 0.2, 1.32],
    ["L", 1.12, 1.32],
  ],
  [
    ["M", 1.05, 0.2],
    ["L", 0.32, 0.2],
    ["L", 0.25, 0.98],
    ["C", 0.5, 0.74, 1.1, 0.8, 1.1, 1.3],
    ["C", 1.1, 1.92, 0.4, 1.95, 0.2, 1.52],
  ],
  [
    ["M", 1.02, 0.38],
    ["C", 0.7, 0.05, 0.2, 0.3, 0.2, 1.1],
    ["C", 0.2, 1.95, 1.1, 1.95, 1.1, 1.36],
    ["C", 1.1, 0.78, 0.3, 0.76, 0.2, 1.2],
  ],
  [
    ["M", 0.2, 0.2],
    ["L", 1.1, 0.2],
    ["L", 0.5, 1.8],
  ],
  [
    ["M", 0.65, 0.96],
    ["C", 0.18, 0.96, 0.12, 0.2, 0.65, 0.2],
    ["C", 1.18, 0.2, 1.12, 0.96, 0.65, 0.96],
    ["C", 0.08, 0.96, 0.05, 1.8, 0.65, 1.8],
    ["C", 1.25, 1.8, 1.22, 0.96, 0.65, 0.96],
    ["Z"],
  ],
]

/** The width of one digit at scale 1, in staff spaces. A digit is 2 high. */
export const DIGIT_WIDTH = 1.3

/** A heavy numeral whose box starts at a top-left corner. */
export function digit(
  value: number,
  x: number,
  top: number,
  sp: number,
  scale = 1
): DrawOp {
  const size = sp * scale
  // A nine is a six turned half way round.
  if (value === 9)
    return stroked(
      place(DIGITS[6], x + DIGIT_WIDTH * size, top + 2 * size, -size),
      0.34 * size
    )
  return stroked(place(DIGITS[value] ?? DIGITS[0], x, top, size), 0.34 * size)
}

/** A whole number as numerals, centred on `cx`. */
export function numerals(
  value: number,
  cx: number,
  top: number,
  sp: number,
  scale = 1
): DrawOp[] {
  const digits = [...String(Math.max(0, Math.round(value)))].map(Number)
  const width = DIGIT_WIDTH * sp * scale
  const left = cx - (digits.length * width) / 2
  return digits.map((value, index) =>
    digit(value, left + index * width, top, sp, scale)
  )
}

/** The brace that joins the two staves of a grand staff. `x` is its right. */
export function brace(
  x: number,
  top: number,
  bottom: number,
  sp: number
): DrawOp {
  const width = 1.5 * sp
  const height = bottom - top
  const middle = (top + bottom) / 2
  return filled([
    ["M", x, top],
    [
      "C",
      x - 0.9 * width,
      top + 0.12 * height,
      x + 0.35 * width,
      middle - 0.14 * height,
      x - width,
      middle,
    ],
    [
      "C",
      x + 0.35 * width,
      middle + 0.14 * height,
      x - 0.9 * width,
      bottom - 0.12 * height,
      x,
      bottom,
    ],
    [
      "C",
      x - 0.5 * width,
      bottom - 0.14 * height,
      x + 0.8 * width,
      middle + 0.14 * height,
      x - width,
      middle,
    ],
    [
      "C",
      x + 0.8 * width,
      middle - 0.14 * height,
      x - 0.5 * width,
      top + 0.14 * height,
      x,
      top,
    ],
    ["Z"],
  ])
}

/** A tie: a thin crescent between two points at one height. */
export function tie(
  x1: number,
  x2: number,
  y: number,
  sp: number,
  above: boolean
): DrawOp {
  const span = x2 - x1
  const inset = span * 0.25
  const height =
    (above ? -1 : 1) * Math.max(0.5 * sp, Math.min(1.1 * sp, span * 0.12))
  return filled([
    ["M", x1, y],
    ["C", x1 + inset, y + height, x2 - inset, y + height, x2, y],
    ["C", x2 - inset, y + height * 0.6, x1 + inset, y + height * 0.6, x1, y],
    ["Z"],
  ])
}
