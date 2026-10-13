import type { Message } from "../types"

export const pluginsEn = {
  "Could not add plugin": "Could not add plugin",
  "Could not scale native plugin content": "Could not scale native plugin content",
  "Drop an effect onto a mixer slot": "Drop an effect onto a mixer slot",
  "Drop an instrument onto a channel": "Drop an instrument onto a channel",
  "Native plugin hosting is unavailable in browser mode": "Native plugin hosting is unavailable in browser mode",
  "The destination channel was removed": "The destination channel was removed",
  "The destination effect slot was removed": "The destination effect slot was removed",
  "The destination mixer track was removed": "The destination mixer track was removed",
  "The project changed during plugin insertion": "The project changed during plugin insertion",
  "The project changed during the plugin drag": "The project changed during the plugin drag",
  "This plugin is no longer in the catalog": "This plugin is no longer in the catalog",
  "This plugin is unavailable": "This plugin is unavailable",
  "Unknown built-in effect": "Unknown built-in effect",
  "Unknown built-in instrument": "Unknown built-in instrument",
  "{input} · {layout}": "{input} · {layout}",
} as const

export const pluginsEs = {
  "Could not add plugin": "No se pudo añadir el plugin",
  "Could not scale native plugin content": "No se pudo escalar el contenido del plugin nativo",
  "Drop an effect onto a mixer slot": "Suelta un efecto sobre una ranura del mezclador",
  "Drop an instrument onto a channel": "Suelta un instrumento sobre un canal",
  "Native plugin hosting is unavailable in browser mode": "El alojamiento de plugins nativos no está disponible en modo navegador",
  "The destination channel was removed": "El canal de destino se ha eliminado",
  "The destination effect slot was removed": "La ranura de efecto de destino se ha eliminado",
  "The destination mixer track was removed": "La pista del mezclador de destino se ha eliminado",
  "The project changed during plugin insertion": "El proyecto cambió mientras se insertaba el plugin",
  "The project changed during the plugin drag": "El proyecto cambió mientras se arrastraba el plugin",
  "This plugin is no longer in the catalog": "Este plugin ya no está en el catálogo",
  "This plugin is unavailable": "Este plugin no está disponible",
  "Unknown built-in effect": "Efecto integrado desconocido",
  "Unknown built-in instrument": "Instrumento integrado desconocido",
  "{input} · {layout}": "{input} · {layout}",
} satisfies Record<keyof typeof pluginsEn, Message>
