import type { Message } from "../types"

export const performanceMediaEn = {
  "Show performance grid": "Show performance grid",
  "Show timeline video": "Show timeline video",
  "Show visual stage": "Show visual stage",
  "The project changed before this media operation": "The project changed before this media operation",
} as const

export const performanceMediaEs = {
  "Show performance grid": "Mostrar la cuadrícula de interpretación",
  "Show timeline video": "Mostrar el vídeo de la línea de tiempo",
  "Show visual stage": "Mostrar el escenario visual",
  "The project changed before this media operation": "El proyecto cambió antes de esta operación multimedia",
} satisfies Record<keyof typeof performanceMediaEn, Message>
