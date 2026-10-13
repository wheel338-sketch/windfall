import type {
  EffectKind,
  EffectParams,
  InstrumentKind,
  InstrumentParams,
  ParamInfo,
} from "@/bindings"
import { effects, instruments } from "@/bindings/descriptors.json"
import { message } from "@/lib/i18n"

/**
 * Everything the UI knows about one kind of effect or instrument: its name,
 * its settings in the order the `set…Param` commands number them, and the
 * settings a new one starts with.
 */
export type ParamDescriptor<Params extends object = object> = {
  readonly name: string
  readonly params: readonly ParamInfo[]
  readonly defaults: Params
}

export type EffectParamsOf<Kind extends EffectKind> = Extract<
  EffectParams,
  { type: Kind }
>
export type InstrumentParamsOf<Kind extends InstrumentKind> = Extract<
  InstrumentParams,
  { type: Kind }
>

/**
 * The shortest decimal that is the same 32-bit float. The core keeps its
 * settings as 32-bit floats. The generated table writes them out at full
 * length (0.699999988079071), while a project writes them the short way
 * (0.7). Read as doubles those two differ, so the table is brought to the
 * project's spelling here: a default then equals the stored value it is
 * compared with, and settings made from defaults read back unchanged.
 */
export function shortFloat(value: number): number {
  if (!Number.isFinite(value)) return value
  const float = Math.fround(value)
  for (let digits = 1; digits < 10; digits += 1) {
    const short = Number(value.toPrecision(digits))
    if (Math.fround(short) === float) return short
  }
  return value
}

function shortened<T>(value: T): T
function shortened(value: unknown): unknown {
  if (typeof value === "number") return shortFloat(value)
  if (Array.isArray(value)) return value.map((item) => shortened(item))
  if (typeof value === "object" && value !== null) {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, shortened(item)])
    )
  }
  return value
}

// The file is generated from the Rust tables that also generate the types,
// so the two agree. JSON has no literal types, which is all the cast adds.
// Provisional append-only descriptor until the combined native generator pass.
const sidechain: ParamInfo = { id: "sidechain", name: "External sidechain", kind: "toggle", unit: "none", scale: "linear", min: 0, max: 1, default: 0, choices: [] }
const EFFECTS = shortened({ ...effects, compressor: { ...effects.compressor, params: effects.compressor.params.some((info) => info.id === "sidechain") ? effects.compressor.params : [...effects.compressor.params, sidechain], defaults: { ...effects.compressor.defaults, sidechain: false } } }) as unknown as {
  readonly [Kind in EffectKind]: ParamDescriptor<EffectParamsOf<Kind>>
}
const INSTRUMENTS = shortened(instruments) as unknown as {
  readonly [Kind in InstrumentKind]: ParamDescriptor<InstrumentParamsOf<Kind>>
}

export const EFFECT_KINDS = Object.keys(EFFECTS) as EffectKind[]
export const INSTRUMENT_KINDS = Object.keys(INSTRUMENTS) as InstrumentKind[]

export function effectDescriptor<Kind extends EffectKind>(
  kind: Kind
): ParamDescriptor<EffectParamsOf<Kind>> {
  return EFFECTS[kind]
}

export function instrumentDescriptor<Kind extends InstrumentKind>(
  kind: Kind
): ParamDescriptor<InstrumentParamsOf<Kind>> {
  return INSTRUMENTS[kind]
}

const indexes = new WeakMap<readonly ParamInfo[], ReadonlyMap<string, number>>()

function indexOf(descriptor: ParamDescriptor): ReadonlyMap<string, number> {
  let map = indexes.get(descriptor.params)
  if (!map) {
    map = new Map(descriptor.params.map((info, index) => [info.id, index]))
    indexes.set(descriptor.params, map)
  }
  return map
}

/**
 * The number the `setEffectParam` and `setInstrumentParam` commands know a
 * setting by: its place in the descriptor's table. Throws for an id the
 * table does not have, so a typo fails at once.
 */
export function paramIndex(descriptor: ParamDescriptor, id: string): number {
  const index = indexOf(descriptor).get(id)
  if (index === undefined) {
    throw new Error(message("{descriptorName} has no parameter \"{id}\"", { descriptorName: descriptor.name, id }))
  }
  return index
}

/** The description of a setting, by id. Throws like `paramIndex`. */
export function paramInfo(descriptor: ParamDescriptor, id: string): ParamInfo {
  return descriptor.params[paramIndex(descriptor, id)]
}
