import type { Message } from "../types"

export const componentsEn = {
  "Clear clip indicator": "Clear clip indicator",
  "Command Palette": "Command Palette",
  "Decay and sustain": "Decay and sustain",
  "Display gain {db}": "Display gain {db}",
  "Loading": "Loading",
  "Piano keyboard": "Piano keyboard",
  "Region end": "Region end",
  "Region start": "Region start",
  "Search for a command to run...": "Search for a command to run...",
  "{live}, automated (set to {stored})": "{live}, automated (set to {stored})",
  "{time}, sustain {percent}": "{time}, sustain {percent}",
} as const

export const componentsEs = {
  "Clear clip indicator": "Borrar el indicador de saturación",
  "Command Palette": "Paleta de comandos",
  "Decay and sustain": "Caída y sostenimiento",
  "Display gain {db}": "Ganancia de visualización {db}",
  "Loading": "Cargando",
  "Piano keyboard": "Teclado de piano",
  "Region end": "Final de la región",
  "Region start": "Inicio de la región",
  "Search for a command to run...": "Busca un comando para ejecutarlo...",
  "{live}, automated (set to {stored})": "{live}, automatizado (ajustado a {stored})",
  "{time}, sustain {percent}": "{time}, sostenimiento {percent}",
} satisfies Record<keyof typeof componentsEn, Message>
