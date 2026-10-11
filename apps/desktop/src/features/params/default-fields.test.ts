import { describe, expect, it } from "vitest"
import { effectDescriptor, paramIndex } from "./descriptors"
import { readParam, writeParam } from "./access"

describe("omitted filter pattern defaults", () => {
  it("reads and edits legacy/default snapshots without mutating or inventing another kind", () => {
    const descriptor = effectDescriptor("filterBank")
    const legacy = { ...descriptor.defaults }
    // The persisted neutral value intentionally has no pattern array.
    const saved = JSON.parse(JSON.stringify(legacy)) as Record<string, unknown>
    delete saved.patterns
    const before = structuredClone(saved)
    const info = descriptor.params[paramIndex(descriptor, "patterns.7.gate.3")]
    expect(readParam(saved, info)).toBe(1)
    const edited = writeParam(saved, info, 0.25)
    expect(readParam(edited, info)).toBe(0.25)
    expect(readParam(edited, descriptor.params[paramIndex(descriptor, "patterns.0.cutoff.0")])).toBe(0)
    expect(saved).toEqual(before)
    expect(() => readParam({ type: "eq" }, info)).toThrow()
  })
})
