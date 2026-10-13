import { describe, expect, it } from "vitest"

import descriptors from "@/bindings/descriptors.json"
import { paramGroups, type ParamDescriptor } from "@/features/params"
import { TRACK_PROCESSING_INFO } from "@/lib/track-processing"
import { catalogs } from "."
import { invariantReason } from "./invariants"

describe("all built-in descriptor localization admission", () => {
  it("requires translated processor names, parameter names, choices and generated group labels", () => {
    const missing = new Set<string>()
    const requireLabel = (address: string, source: string) => {
      if (invariantReason(source)) return
      for (const [language, catalog] of Object.entries(catalogs))
        if (!Object.hasOwn(catalog, source)) missing.add(`${language}: ${address}: ${source}`)
    }
    let parameters = 0
    for (const [category, table] of Object.entries({ effect: descriptors.effects, instrument: descriptors.instruments }))
      for (const [kind, descriptor] of Object.entries(table)) {
        requireLabel(`${category}.${kind}.name`, descriptor.name)
        for (const info of descriptor.params) {
          parameters += 1
          requireLabel(`${category}.${kind}.${info.id}.name`, info.name)
          for (const choice of info.choices) requireLabel(`${category}.${kind}.${info.id}.choice.${choice.value}`, choice.label)
        }
        for (const group of paramGroups(descriptor as unknown as ParamDescriptor)) {
          if (group.title) requireLabel(`${category}.${kind}.group.${group.key}`, group.title)
          for (const param of group.params) requireLabel(`${category}.${kind}.${param.info.id}.short`, param.label)
        }
      }
    for (const info of TRACK_PROCESSING_INFO) requireLabel(`track.${info.id}`, info.name)
    expect(parameters).toBeGreaterThan(2600)
    expect([...missing].sort()).toEqual([])
  })
})
