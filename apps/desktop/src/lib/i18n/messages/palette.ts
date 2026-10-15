import type { Message } from "../types"

export const paletteEn = {
  "Closest matches": "Closest matches",
} as const

export const paletteEs = {
  "Closest matches": "Coincidencias más cercanas",
} satisfies Record<keyof typeof paletteEn, Message>
