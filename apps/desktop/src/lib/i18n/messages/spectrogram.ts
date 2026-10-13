import type { Message } from "../types"

export const spectrogramEn = {
  "No spectrogram available": "No spectrogram available",
  "Oldest at top · Newest at bottom": "Oldest at top · Newest at bottom",
  "Short history of engine power. Relative levels without hardware calibration.": "Short history of engine power. Relative levels without hardware calibration.",
  "Show spectrogram": "Show spectrogram",
  "Spectrogram, DC to Nyquist from left to right, oldest to newest from top to bottom": "Spectrogram, DC to Nyquist from left to right, oldest to newest from top to bottom",
} as const

export const spectrogramEs = {
  "No spectrogram available": "No hay espectrograma disponible",
  "Oldest at top · Newest at bottom": "Lo más antiguo arriba · Lo más reciente abajo",
  "Short history of engine power. Relative levels without hardware calibration.": "Historial breve de la potencia del motor. Niveles relativos sin calibración de hardware.",
  "Show spectrogram": "Mostrar espectrograma",
  "Spectrogram, DC to Nyquist from left to right, oldest to newest from top to bottom": "Espectrograma, de DC a Nyquist de izquierda a derecha, de lo más antiguo a lo más reciente de arriba abajo",
} satisfies Record<keyof typeof spectrogramEn, Message>
