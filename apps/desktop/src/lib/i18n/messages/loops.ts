import type { Message } from "../types"

export const loopsEn = {
  "All genres": "All genres",
  "All keys": "All keys",
  "BPM with independent key fit.": "BPM with independent key fit.",
  "Change genre, key or tempo filters. The desktop app generates the factory library.": "Change genre, key or tempo filters. The desktop app generates the factory library.",
  "Could not insert the factory loop": "Could not insert the factory loop",
  "Insert": "Insert",
  "Insert key": "Insert key",
  "Keep source key": "Keep source key",
  "Maximum BPM": "Maximum BPM",
  "Minimum BPM": "Minimum BPM",
  "No matching loops": "No matching loops",
  "Original synthesized CC0 content. Audition at the source tempo; Insert or drag to the playlist resynthesizes at": "Original synthesized CC0 content. Audition at the source tempo; Insert or drag to the playlist resynthesizes at",
  "Provenance": "Provenance",
  "Source key": "Source key",
  "Windfall starter loops": "Windfall starter loops",
  "{key} minor": "{key} minor",
} as const

export const loopsEs = {
  "All genres": "Todos los géneros",
  "All keys": "Todas las tonalidades",
  "BPM with independent key fit.": "BPM con ajuste de tonalidad independiente.",
  "Change genre, key or tempo filters. The desktop app generates the factory library.": "Cambia los filtros de género, tonalidad o tempo. La aplicación de escritorio genera la biblioteca de fábrica.",
  "Could not insert the factory loop": "No se pudo insertar el bucle de fábrica",
  "Insert": "Insertar",
  "Insert key": "Tonalidad de inserción",
  "Keep source key": "Conservar la tonalidad de origen",
  "Maximum BPM": "BPM máximo",
  "Minimum BPM": "BPM mínimo",
  "No matching loops": "Ningún bucle coincide",
  "Original synthesized CC0 content. Audition at the source tempo; Insert or drag to the playlist resynthesizes at": "Contenido original sintetizado con licencia CC0. Escúchalo al tempo de origen; al insertarlo o arrastrarlo a la lista de reproducción se vuelve a sintetizar a",
  "Provenance": "Procedencia",
  "Source key": "Tonalidad de origen",
  "Windfall starter loops": "Bucles iniciales de Windfall",
  "{key} minor": "{key} menor",
} satisfies Record<keyof typeof loopsEn, Message>
