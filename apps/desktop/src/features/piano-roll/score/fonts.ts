/*
 * Advance widths of the four standard PDF fonts the score uses, in 1/1000 of
 * the font size, for the printable ASCII range. They place centred and
 * right-aligned text in the PDF, where nothing is embedded, and bound text in
 * the layout. The SVG renderer asks for the matching system faces.
 */

export type FontId = "serif" | "serif-bold" | "serif-italic" | "sans"

export const FONTS: Record<
  FontId,
  { pdf: string; css: string; weight: string; style: string }
> = {
  serif: {
    pdf: "Times-Roman",
    css: "'Times New Roman', Times, serif",
    weight: "normal",
    style: "normal",
  },
  "serif-bold": {
    pdf: "Times-Bold",
    css: "'Times New Roman', Times, serif",
    weight: "bold",
    style: "normal",
  },
  "serif-italic": {
    pdf: "Times-Italic",
    css: "'Times New Roman', Times, serif",
    weight: "normal",
    style: "italic",
  },
  sans: {
    pdf: "Helvetica",
    css: "Helvetica, Arial, sans-serif",
    weight: "normal",
    style: "normal",
  },
}

// Characters 32 to 126.
const WIDTHS: Record<FontId, readonly number[]> = {
  serif: [
    250, 333, 408, 500, 500, 833, 778, 180, 333, 333, 500, 564, 250, 333, 250,
    278, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 278, 278, 564, 564,
    564, 444, 921, 722, 667, 667, 722, 611, 556, 722, 722, 333, 389, 722, 611,
    889, 722, 722, 556, 722, 667, 556, 611, 722, 722, 944, 722, 722, 611, 333,
    278, 333, 469, 500, 333, 444, 500, 444, 500, 444, 333, 500, 500, 278, 278,
    500, 278, 778, 500, 500, 500, 500, 333, 389, 278, 500, 500, 722, 500, 500,
    444, 480, 200, 480, 541,
  ],
  "serif-bold": [
    250, 333, 555, 500, 500, 1000, 833, 278, 333, 333, 500, 570, 250, 333, 250,
    278, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 333, 333, 570, 570,
    570, 500, 930, 722, 667, 722, 722, 667, 611, 778, 778, 389, 500, 778, 667,
    944, 722, 778, 611, 778, 722, 556, 667, 722, 722, 1000, 722, 722, 667, 333,
    278, 333, 581, 500, 333, 500, 556, 444, 556, 444, 333, 500, 556, 278, 333,
    556, 278, 833, 556, 500, 556, 556, 444, 389, 333, 556, 500, 722, 500, 500,
    444, 394, 220, 394, 520,
  ],
  "serif-italic": [
    250, 333, 420, 500, 500, 833, 778, 214, 333, 333, 500, 675, 250, 333, 250,
    278, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 333, 333, 675, 675,
    675, 500, 920, 611, 611, 667, 722, 611, 611, 722, 722, 333, 444, 667, 556,
    833, 667, 722, 611, 722, 611, 500, 556, 722, 611, 833, 611, 556, 556, 389,
    278, 389, 422, 500, 333, 500, 500, 444, 500, 444, 278, 500, 500, 278, 278,
    444, 278, 722, 500, 500, 500, 500, 389, 389, 278, 500, 444, 667, 444, 444,
    389, 400, 275, 400, 541,
  ],
  sans: [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278,
    278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584,
    584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556,
    833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278,
    278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222,
    500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500,
    500, 334, 260, 334, 584,
  ],
}

/** The widest glyph any of the four fonts has, for characters not listed. */
const WIDE = 1015

function charWidth(character: string, font: FontId): number {
  const code = character.codePointAt(0) ?? 32
  if (code >= 32 && code <= 126) return WIDTHS[font][code - 32]
  // An accented letter is as wide as its base letter.
  const base = character.normalize("NFD").codePointAt(0) ?? 0
  if (base !== code && base >= 32 && base <= 126) return WIDTHS[font][base - 32]
  return WIDE
}

/** An upper bound on the width of a line of text, in points. */
export function textWidth(text: string, font: FontId, size: number): number {
  let total = 0
  for (const character of text) total += charWidth(character, font)
  return (total * size) / 1000
}

/** Shrinks, then shortens, a line until it fits a width. */
export function fitText(
  text: string,
  font: FontId,
  size: number,
  width: number,
  smallest = 8
): { text: string; size: number } {
  let fitted = size
  while (fitted > smallest && textWidth(text, font, fitted) > width)
    fitted -= 0.5
  if (textWidth(text, font, fitted) <= width) return { text, size: fitted }
  const characters = [...text]
  while (
    characters.length &&
    textWidth(characters.join("") + "...", font, fitted) > width
  )
    characters.pop()
  return { text: characters.join("").trimEnd() + "...", size: fitted }
}

/** Windows-1252 codes for the characters it has outside Latin-1. */
const CP1252: Record<number, number> = {
  0x20ac: 128, 0x201a: 130, 0x0192: 131, 0x201e: 132, 0x2026: 133,
  0x2020: 134, 0x2021: 135, 0x02c6: 136, 0x2030: 137, 0x0160: 138,
  0x2039: 139, 0x0152: 140, 0x017d: 142, 0x2018: 145, 0x2019: 146,
  0x201c: 147, 0x201d: 148, 0x2022: 149, 0x2013: 150, 0x2014: 151,
  0x02dc: 152, 0x2122: 153, 0x0161: 154, 0x203a: 155, 0x0153: 156,
  0x017e: 158, 0x0178: 159,
}

/**
 * The WinAnsi byte for each character of a line, for the standard PDF fonts.
 * A character the encoding lacks becomes a question mark.
 */
export function winAnsiCodes(text: string): number[] {
  const codes: number[] = []
  for (const character of text.normalize("NFC")) {
    const code = character.codePointAt(0) ?? 63
    if (code >= 32 && code <= 126) codes.push(code)
    else if (code >= 160 && code <= 255) codes.push(code)
    else codes.push(CP1252[code] ?? 63)
  }
  return codes
}
