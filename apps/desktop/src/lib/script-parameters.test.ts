import { describe, expect, it } from "vitest"
import type { Project } from "@/bindings"
import { targetState, describeTarget } from "@/lib/automation/targets"
import { scriptParameterTargets, SCRIPT_PARAMETER_BASE } from "./script-parameters"

const settings = { name: "Script parameters", tempoBpm: 120, timeSignature: { numerator: 4, denominator: 4 }, swing: 0, scripts: { parameterAddresses: [{ channel: 2, script: "live", name: "transpose" }, { channel: 2, script: "retired", name: "old" }],
  assets: [{ reference: { id: "live", revision: 2 }, languageVersion: 1, kind: "noteTransform", name: "Live", source: "# checked revision\nparam gain 0 1 1\nparam transpose -12 12 0 # units\nemit" }],
  channels: [{ channel: 2, script: { id: "live", revision: 2 }, parameters: [0.5, 7] }], controllers: [] } } as Project["settings"]

describe("saved script parameter namespace", () => {
  it("keeps saved indices across declaration order changes and excludes tombstones", () => {
    const project = { settings, channels: [{ id: 2, name: "Lead" }], mixer: { tracks: [] } } as unknown as Project
    const targets = scriptParameterTargets(project)
    expect(targets).toEqual([{ type: "instrumentParam", channel: 2, param: SCRIPT_PARAMETER_BASE }])
    expect(targetState(project, targets[0])).toMatchObject({ stored: 7, range: { min: -12, max: 12, taper: "linear" } })
    expect(describeTarget(project, targets[0])).toBe("Lead script: transpose")
    expect(targetState(project, { type: "instrumentParam", channel: 2, param: SCRIPT_PARAMETER_BASE + 1 })).toBeNull()
  })
})
