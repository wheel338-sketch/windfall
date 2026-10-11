/** Persisted optional musical fields are materialized only for parameter editing.
 * The Rust file/snapshot serializer omits an untouched filter-pattern bank.
 * These immutable defaults are shared; writeParam copies the changed path.
 */
const FILTER_PATTERNS = Array.from({ length: 8 }, () => ({
  enabled: false,
  beats: 4,
  depthOctaves: 2,
  gate: Array.from({ length: 8 }, () => 1),
  cutoff: Array.from({ length: 8 }, () => 0),
}))

export function defaultFields<Params extends object>(params: Params): Params {
  if ("type" in params && params.type === "filterBank" && !("patterns" in params)) {
    return { ...params, patterns: FILTER_PATTERNS }
  }
  return params
}
