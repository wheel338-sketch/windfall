import { describe, expect, it } from "vitest"

import type { AutomationTarget, Project } from "@/bindings"
import fixtures from "@/bindings/automation-fixtures.json"
import { SimDocument } from "@/lib/ipc/sim/document"
import { buildProject, demoProject } from "@/lib/ipc/sim/project"

import {
  automationBlocked,
  automationsOf,
  describeTarget,
  storedNormalized,
  targetState,
  targetValue,
} from "./targets"

/** The demo beat with a synth, a send and two effects, and their ids. */
function song() {
  const ids = { synth: 0, kickTrack: 0, bus: 0, reverb: 0, limiter: 0 }
  const base = demoProject()
  const project = buildProject(base, (run) => {
    ids.kickTrack = base.mixer.tracks[1].id
    ;[ids.synth] = run({ type: "addChannel", instrument: "subtractiveSynth" })
    ;[ids.bus] = run({ type: "addMixerTrack", name: "Space" })
    run({ type: "setSend", from: ids.kickTrack, to: ids.bus, gain: 0.5 })
    ;[ids.reverb] = run({ type: "addEffect", track: ids.bus, kind: "reverb" })
    run({
      type: "updateEffect",
      track: ids.bus,
      effect: ids.reverb,
      patch: { mix: 0.3 },
    })
    ;[ids.limiter] = run({ type: "addEffect", track: 0, kind: "limiter" })
  })
  return { project, ids, kick: base.channels[0] }
}

describe("targetState", () => {
  it("gives the range and the stored value of every kind of target", () => {
    const { project, ids, kick } = song()
    const states: [AutomationTarget, string, number][] = [
      [{ type: "channelVolume", channel: kick.id }, "square", kick.volume],
      [{ type: "channelPan", channel: kick.id }, "linear", 0],
      [{ type: "trackVolume", track: ids.kickTrack }, "square", 1],
      [{ type: "trackPan", track: ids.kickTrack }, "linear", 0],
      [
        { type: "sendGain", track: ids.kickTrack, target: ids.bus },
        "square",
        0.5,
      ],
      [
        { type: "effectMix", track: ids.bus, effect: ids.reverb },
        "linear",
        0.3,
      ],
      [{ type: "tempo" }, "linear", 128],
    ]
    for (const [target, taper, stored] of states) {
      const state = targetState(project, target)
      expect(state?.range.taper, target.type).toBe(taper)
      expect(state?.stored, target.type).toBeCloseTo(stored, 6)
      expect(state?.info).toBeNull()
    }
  })

  it("reads a setting of an effect and of an instrument by its index", () => {
    const { project, ids } = song()
    const synth = project.channels.find((item) => item.id === ids.synth)!
    expect(synth.source.type).toBe("instrument")
    // Every setting, checked against the value the project stores for it.
    const settings =
      synth.source.type === "instrument" ? synth.source.params : null
    let cutoff = -1
    for (let param = 0; param < 55; param += 1) {
      const state = targetState(project, {
        type: "instrumentParam",
        channel: ids.synth,
        param,
      })
      expect(state, `setting ${param}`).not.toBeNull()
      expect(state!.stored).toBeGreaterThanOrEqual(state!.range.min)
      expect(state!.stored).toBeLessThanOrEqual(state!.range.max)
      if (state!.info?.id === "filter.cutoffHz") cutoff = param
    }
    expect(settings).not.toBeNull()
    const state = targetState(project, {
      type: "instrumentParam",
      channel: ids.synth,
      param: cutoff,
    })
    expect(state).toMatchObject({
      range: { min: 20, max: 20000, taper: "logarithmic" },
      stored: 20000,
    })

    const decay = targetState(project, {
      type: "effectParam",
      track: ids.bus,
      effect: ids.reverb,
      param: 0,
    })
    expect(decay?.info).not.toBeNull()
    expect(decay?.stored).toBeGreaterThan(0)
  })

  it("agrees with the engine's ranges for every setting there is", () => {
    // The fixtures name each range "<Processor>: <id of the setting>".
    const { project, ids } = song()
    const named = new Map(
      fixtures.ranges.map((entry) => [entry.name, entry.range])
    )
    const check = (
      processor: string,
      target: (param: number) => AutomationTarget
    ) => {
      let checked = 0
      for (let param = 0; param < 80; param += 1) {
        const state = targetState(project, target(param))
        if (!state?.info) break
        const range = named.get(`${processor}: ${state.info.id}`)
        expect(range, `${processor}: ${state.info.id}`).toBeDefined()
        expect(state.range.taper).toBe(range?.taper)
        expect(state.range.min).toBeCloseTo(range?.min ?? NaN, 5)
        expect(state.range.max).toBeCloseTo(range?.max ?? NaN, 3)
        checked += 1
      }
      return checked
    }
    expect(
      check("Reverb", (param) => ({
        type: "effectParam",
        track: ids.bus,
        effect: ids.reverb,
        param,
      }))
    ).toBe(11)
    expect(
      check("Limiter", (param) => ({
        type: "effectParam",
        track: 0,
        effect: ids.limiter,
        param,
      }))
    ).toBe(11)
    expect(
      check("Subtractive synth", (param) => ({
        type: "instrumentParam",
        channel: ids.synth,
        param,
      }))
    ).toBe(55)
  })

  it("is null for what the project does not have", () => {
    const { project, ids, kick } = song()
    const missing: AutomationTarget[] = [
      { type: "channelVolume", channel: 9999 },
      { type: "trackPan", track: 9999 },
      { type: "sendGain", track: ids.kickTrack, target: 9999 },
      { type: "effectMix", track: ids.bus, effect: 9999 },
      { type: "effectParam", track: ids.bus, effect: ids.reverb, param: 999 },
      // A sampler has no instrument settings.
      { type: "instrumentParam", channel: kick.id, param: 0 },
      { type: "instrumentParam", channel: ids.synth, param: 999 },
    ]
    for (const target of missing) {
      expect(targetState(project, target), target.type).toBeNull()
    }
  })

  it("matches the value the document gives a new automation's first point", () => {
    const { project, ids, kick } = song()
    const document = SimDocument.create(project)
    const targets: AutomationTarget[] = [
      { type: "channelVolume", channel: kick.id },
      { type: "sendGain", track: ids.kickTrack, target: ids.bus },
      { type: "effectMix", track: ids.bus, effect: ids.reverb },
      { type: "effectParam", track: ids.bus, effect: ids.reverb, param: 0 },
      { type: "instrumentParam", channel: ids.synth, param: 3 },
      { type: "tempo" },
    ]
    for (const target of targets) {
      document.dispatch({ type: "addAutomation", target })
      const made = document.project().automations.at(-1)!
      expect(storedNormalized(project, target), target.type).toBeCloseTo(
        made.points[0].value,
        5
      )
    }
    document.dispose()
  })
})

describe("targetValue", () => {
  it("is the automation value in the target's own unit", () => {
    const { project, ids } = song()
    const fader: AutomationTarget = {
      type: "trackVolume",
      track: ids.kickTrack,
    }
    expect(targetValue(project, fader, Math.SQRT1_2)).toBeCloseTo(1, 9)
    expect(targetValue(project, fader, 1)).toBe(2)
    expect(targetValue(project, { type: "tempo" }, 0)).toBe(10)
    expect(
      targetValue(project, { type: "trackPan", track: 9999 }, 0)
    ).toBeNull()
  })
})

describe("describeTarget", () => {
  it("says what an automation moves", () => {
    const { project, ids, kick } = song()
    const said = (target: AutomationTarget) => describeTarget(project, target)
    expect(said({ type: "channelVolume", channel: kick.id })).toBe(
      "Kick → volume"
    )
    expect(said({ type: "channelPan", channel: kick.id })).toBe("Kick → pan")
    expect(said({ type: "trackVolume", track: ids.bus })).toBe("Space → volume")
    expect(said({ type: "trackPan", track: 0 })).toBe("Master → pan")
    expect(
      said({ type: "sendGain", track: ids.kickTrack, target: ids.bus })
    ).toBe("Kick → send to Space")
    expect(
      said({ type: "effectMix", track: ids.bus, effect: ids.reverb })
    ).toBe("Reverb · Mix")
    expect(
      said({
        type: "effectParam",
        track: ids.bus,
        effect: ids.reverb,
        param: 0,
      })
    ).toMatch(/^Reverb · \w/)
    expect(said({ type: "tempo" })).toBe("Tempo")
  })

  it("names a setting of an instrument after its channel", () => {
    const { project, ids } = song()
    const name = project.channels.find((item) => item.id === ids.synth)!.name
    expect(
      describeTarget(project, {
        type: "instrumentParam",
        channel: ids.synth,
        param: 0,
      })
    ).toMatch(new RegExp(`^${name} · \\w`))
  })

  it("still has words for a target that is gone", () => {
    const { project } = song()
    expect(describeTarget(project, { type: "trackPan", track: 9999 })).toBe(
      "Removed track → pan"
    )
    expect(
      describeTarget(project, { type: "effectMix", track: 0, effect: 9999 })
    ).toBe("Removed effect · Mix")
  })
})

describe("automationsOf", () => {
  it("finds the automations of a target, however its track is named", () => {
    const { project, ids } = song()
    const document = SimDocument.create(project)
    const mix: AutomationTarget = {
      type: "effectMix",
      track: ids.bus,
      effect: ids.reverb,
    }
    document.dispatch({ type: "addAutomation", target: mix })
    document.dispatch({ type: "addAutomation", target: { type: "tempo" } })
    document.dispatch({ type: "addAutomation", target: mix })
    const all: Project["automations"] = document.project().automations
    expect(automationsOf(all, mix).map((item) => item.id)).toEqual([
      all[0].id,
      all[2].id,
    ])
    expect(automationsOf(all, { type: "tempo" })).toHaveLength(1)
    expect(automationsOf(all, { type: "trackPan", track: 0 })).toEqual([])
    document.dispose()
  })
})

describe("automationBlocked", () => {
  it("is the limiter's look-ahead, which the engine leaves alone", () => {
    const { project, ids } = song()
    let lookahead = -1
    for (let param = 0; param < 20; param += 1) {
      const target: AutomationTarget = {
        type: "effectParam",
        track: 0,
        effect: ids.limiter,
        param,
      }
      const state = targetState(project, target)
      if (!state) break
      const blocked = automationBlocked(project, target)
      if (state.info?.id === "lookaheadMs") {
        lookahead = param
        expect(blocked).toBe("Changes the latency")
      } else {
        expect(blocked, state.info?.id).toBeNull()
      }
    }
    expect(lookahead).toBeGreaterThanOrEqual(0)
    expect(automationBlocked(project, { type: "tempo" })).toBeNull()
  })
})
