import type { Message } from "../types"

export const notebookEn = {
  "Adding your changes to the song…": "Adding your changes to the song…",
  "Fix the marked text to add your changes to the song.": "Fix the marked text to add your changes to the song.",
  "Notes are part of the song. Save the project to keep them.": "Notes are part of the song. Save the project to keep them.",
  "Page starts at song position": "Page starts at song position",
  "Type bar:beat or bar:beat:tick, such as 9:1. Leave blank for a manual page. Playback, seeks and loops select the latest cue in song mode.": "Type bar:beat or bar:beat:tick, such as 9:1. Leave blank for a manual page. Playback, seeks and loops select the latest cue in song mode.",
} as const

export const notebookEs = {
  "Adding your changes to the song…": "Añadiendo tus cambios a la canción…",
  "Fix the marked text to add your changes to the song.": "Corrige el texto marcado para añadir tus cambios a la canción.",
  "Notes are part of the song. Save the project to keep them.": "Las notas forman parte de la canción. Guarda el proyecto para conservarlas.",
  "Page starts at song position": "La página comienza en la posición de la canción",
  "Type bar:beat or bar:beat:tick, such as 9:1. Leave blank for a manual page. Playback, seeks and loops select the latest cue in song mode.": "Escribe compás:pulso o compás:pulso:tick, por ejemplo 9:1. Déjalo vacío para una página manual. La reproducción, los saltos y los bucles eligen la última posición en modo Canción.",
} satisfies Record<keyof typeof notebookEn, Message>
