import type { Message } from "../types"

export const updatesEn = {
  "Check for updates…": "Check for updates…",
  "Check for updates automatically": "Check for updates automatically",
  "Once a day when Windfall starts, in the background. Nothing is downloaded until you choose to install.": "Once a day when Windfall starts, in the background. Nothing is downloaded until you choose to install.",
  "Update available": "Update available",
  "Windfall {version} is available": "Windfall {version} is available",
  "Install now": "Install now",
  "Later": "Later",
  "Could not check for updates": "Could not check for updates",
} as const satisfies Record<string, Message>

export const updatesEs = {
  "Check for updates…": "Buscar actualizaciones…",
  "Check for updates automatically": "Buscar actualizaciones automáticamente",
  "Once a day when Windfall starts, in the background. Nothing is downloaded until you choose to install.": "Una vez al día, al iniciar Windfall y en segundo plano. No se descarga nada hasta que decidas instalar.",
  "Update available": "Actualización disponible",
  "Windfall {version} is available": "Windfall {version} está disponible",
  "Install now": "Instalar ahora",
  "Later": "Más tarde",
  "Could not check for updates": "No se han podido buscar actualizaciones",
} as const satisfies Record<keyof typeof updatesEn, Message>
