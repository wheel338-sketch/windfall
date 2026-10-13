import type { Message } from "../types"

export const spectrumEn = {
  "Current engine spectrum. Relative power, without history or hardware calibration.": "Current engine spectrum. Relative power, without history or hardware calibration.",
  "Current spectrum, low to high frequency": "Current spectrum, low to high frequency",
  "No spectrum available": "No spectrum available",
} as const

export const spectrumEs = {
  "Current engine spectrum. Relative power, without history or hardware calibration.": "Espectro actual del motor. Potencia relativa, sin historial ni calibración de hardware.",
  "Current spectrum, low to high frequency": "Espectro actual, de frecuencias graves a agudas",
  "No spectrum available": "No hay espectro disponible",
} satisfies Record<keyof typeof spectrumEn, Message>
