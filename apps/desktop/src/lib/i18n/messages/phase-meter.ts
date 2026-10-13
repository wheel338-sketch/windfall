import type { Message } from "../types"

export const phaseMeterEn = {
  "Current stereo correlation.": "Current stereo correlation.",
  "Mid · +1": "Mid · +1",
  "No correlation available": "No correlation available",
  "Phase meter": "Phase meter",
  "Show phase meter": "Show phase meter",
  "Side · −1": "Side · −1",
  "Stereo correlation": "Stereo correlation",
} as const

export const phaseMeterEs = {
  "Current stereo correlation.": "Correlación estéreo actual.",
  "Mid · +1": "Centro · +1",
  "No correlation available": "No hay correlación disponible",
  "Phase meter": "Medidor de fase",
  "Show phase meter": "Mostrar el medidor de fase",
  "Side · −1": "Lateral · −1",
  "Stereo correlation": "Correlación estéreo",
} satisfies Record<keyof typeof phaseMeterEn, Message>
