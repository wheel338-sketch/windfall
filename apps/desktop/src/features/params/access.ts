import type { ParamInfo } from "@/bindings"
import { defaultFields } from "./default-fields"
import { message, translatedText as t } from "@/lib/i18n"

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null
}

function missing(info: ParamInfo): Error {
  return new Error(message("These settings have no \"{infoId}\" ({infoName})", { infoId: info.id, infoName: t(info.name) }))
}

/** Brings a value into the range of its setting, as the core does. */
export function clampParam(info: ParamInfo, value: number): number {
  if (Number.isNaN(value)) return info.default
  const clamped = Math.min(info.max, Math.max(info.min, value))
  return info.kind === "float" ? clamped : Math.round(clamped)
}

/**
 * The value of one setting inside a settings object, as the number the
 * `set…Param` commands take: a toggle is 0 or 1 and a choice is the index of
 * the stored option. Throws when the object has nothing at the setting's id,
 * which means it belongs to another kind.
 */
export function readParam(params: object, info: ParamInfo): number {
  let at: unknown = defaultFields(params)
  for (const key of info.id.split(".")) {
    if (!isRecord(at) || !(key in at)) throw missing(info)
    at = at[key]
  }
  if (typeof at === "number") return at
  if (typeof at === "boolean") return at ? 1 : 0
  if (typeof at === "string") {
    const index = info.choices.findIndex((choice) => choice.value === at)
    return index < 0 ? info.default : index
  }
  throw missing(info)
}

function stored(info: ParamInfo, value: number): number | boolean | string {
  const clamped = clampParam(info, value)
  if (info.kind === "toggle") return clamped >= 0.5
  if (info.kind === "choice") {
    return (info.choices[clamped] ?? info.choices[info.default]).value
  }
  return clamped
}

function writeAt(
  at: unknown,
  keys: string[],
  value: unknown,
  info: ParamInfo
): unknown {
  if (keys.length === 0) return value
  const [key, ...rest] = keys
  if (!isRecord(at) || !(key in at)) throw missing(info)
  const next = writeAt(at[key], rest, value, info)
  if (Array.isArray(at)) {
    const copy: unknown[] = at.slice()
    copy[Number(key)] = next
    return copy
  }
  return { ...at, [key]: next }
}

/**
 * A copy of the settings with one of them changed, for `set…Params` and for
 * showing an edit before the core has answered. `value` is given the way
 * `readParam` returns it and is brought into the setting's range. Whatever
 * the change does not touch is shared with the original.
 */
export function writeParam<Params extends object>(
  params: Params,
  info: ParamInfo,
  value: number
): Params {
  return writeAt(
    defaultFields(params),
    info.id.split("."),
    stored(info, value),
    info
  ) as Params
}
