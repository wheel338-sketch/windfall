import type { Message } from "../types"

export const paramsEn = {
  "Each editor is made from a descriptor alone": "Each editor is made from a descriptor alone",
  "Parameter editors": "Parameter editors",
  "These settings have no \"{infoId}\" ({infoName})": "These settings have no \"{infoId}\" ({infoName})",
  "settings": "settings",
  "{descriptorName} has no parameter \"{id}\"": "{descriptorName} has no parameter \"{id}\"",
} as const

export const paramsEs = {
  "Each editor is made from a descriptor alone": "Cada editor se crea solo a partir de un descriptor",
  "Parameter editors": "Editores de parámetros",
  "These settings have no \"{infoId}\" ({infoName})": "Estos ajustes no tienen \"{infoId}\" ({infoName})",
  "settings": "ajustes",
  "{descriptorName} has no parameter \"{id}\"": "{descriptorName} no tiene el parámetro \"{id}\"",
} satisfies Record<keyof typeof paramsEn, Message>
