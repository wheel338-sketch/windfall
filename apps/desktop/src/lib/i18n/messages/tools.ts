import type { Message } from "../types"

export const toolsEn = {
  "Mute empty playlist tracks": "Mute empty playlist tracks",
  "Reset audio clip levels": "Reset audio clip levels",
  "Set all audio clips to spectral": "Set all audio clips to spectral",
  "Set all audio clips to tape": "Set all audio clips to tape",
  "Unmute playlist clips": "Unmute playlist clips",
  "Unsolo playlist tracks": "Unsolo playlist tracks",
} as const

export const toolsEs = {
  "Mute empty playlist tracks": "Silenciar pistas vacías de la lista de reproducción",
  "Reset audio clip levels": "Restablecer niveles de los clips de audio",
  "Set all audio clips to spectral": "Poner todos los clips de audio en modo espectral",
  "Set all audio clips to tape": "Poner todos los clips de audio en modo cinta",
  "Unmute playlist clips": "Quitar silencio de los clips de la lista de reproducción",
  "Unsolo playlist tracks": "Quitar solo de las pistas de la lista de reproducción",
} satisfies Record<keyof typeof toolsEn, Message>
