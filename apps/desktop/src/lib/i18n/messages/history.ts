import type { Message } from "../types"

export const historyEn = {
  "Click a step to go back or forward to it.": "Click a step to go back or forward to it.",
  "Filter history": "Filter history",
  "Fold": "Fold",
  "History: every edit, in order. Click one to go back to it": "History: every edit, in order. Click one to go back to it",
  "No steps match.": "No steps match.",
  "Nothing to undo yet. Edits will be listed here.": "Nothing to undo yet. Edits will be listed here.",
  "Project opened": "Project opened",
  "Steps of {runLabel}": "Steps of {runLabel}",
  "Undo history": "Undo history",
  "Unfold": "Unfold",
  "{open} the {count} steps of {runLabel}": "{open} the {count} steps of {runLabel}",
} as const

export const historyEs = {
  "Click a step to go back or forward to it.": "Haz clic en un paso para retroceder o avanzar hasta él.",
  "Filter history": "Filtrar el historial",
  "Fold": "Plegar",
  "History: every edit, in order. Click one to go back to it": "Historial: todas las ediciones, en orden. Haz clic en una para volver a ella",
  "No steps match.": "Ningún paso coincide.",
  "Nothing to undo yet. Edits will be listed here.": "Aún no hay nada que deshacer. Las ediciones aparecerán aquí.",
  "Project opened": "Proyecto abierto",
  "Steps of {runLabel}": "Pasos de {runLabel}",
  "Undo history": "Historial de deshacer",
  "Unfold": "Desplegar",
  "{open} the {count} steps of {runLabel}": "{open} the {count} steps of {runLabel}",
} satisfies Record<keyof typeof historyEn, Message>
