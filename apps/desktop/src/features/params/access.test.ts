import { describe, expect, it } from "vitest"

import type { EffectKind, InstrumentKind, ParamInfo } from "@/bindings"
import tables from "@/bindings/descriptors.json"
import { buildProject, emptyProject } from "@/lib/ipc/sim/project"

import { clampParam, readParam, writeParam } from "./access"
import {
  EFFECT_KINDS,
  effectDescriptor,
  INSTRUMENT_KINDS,
  instrumentDescriptor,
  paramIndex,
  paramInfo,
  shortFloat,
  type ParamDescriptor,
} from "./descriptors"

const ALL: [string, ParamDescriptor][] = [
  ...EFFECT_KINDS.map((kind): [string, ParamDescriptor] => [
    kind,
    effectDescriptor(kind),
  ]),
  ...INSTRUMENT_KINDS.map((kind): [string, ParamDescriptor] => [
    kind,
    instrumentDescriptor(kind),
  ]),
]

/** A few values across a setting's range, the ends included. */
function samples(info: ParamInfo): number[] {
  const { min, max } = info
  if (info.kind === "toggle") return [0, 1]
  if (info.kind === "choice") return info.choices.map((_, index) => index)
  const inside = [0.25, 0.5, 0.8].map((share) => min + share * (max - min))
  const values = [min, ...inside, max, info.default]
  return info.kind === "integer" ? values.map(Math.round) : values
}

describe("descriptors", () => {
  it("has one for every kind the bindings name", () => {
    const effects: Record<EffectKind, true> = {
      eq: true,
      compressor: true,
      limiter: true,
      reverb: true,
      delay: true,
      balance: true,
      dcBlock: true,
      channelMute: true,
      polarity: true,
      stereoMatrix: true,
      softClipper: true,
      distortion: true,
      fastLowpass: true,
      selectableFilter: true,
      bassShelf: true,
      lofi: true,
      chorus: true,
      flanger: true,
      phaser: true,
      bandSplit: true,
      multibandCompressor: true,
      multibandMaximizer: true,
      transientShaper: true,
      transientSplit: true,
      oneKnob: true,
      bassHarmonics: true,
      exciter: true,
      waveshaper: true,
      overdrive: true,
      guitarRack: true,
      driveChain: true,
      vintageChorus: true,
      hyperChorus: true,
      vintagePhaser: true,
      stackedFlanger: true,
      bandDelay: true,
      room: true,
      spreader: true,
      stereoEnhancer: true,
      volumeGate: true,
      timeTransport: true,
      scratch: true,
      performanceRack: true,
      convolver: true,
      frequencyShifter: true,
      pitchShift: true,
      pitchCorrect: true,
      vocoder: true,
      echoBank: true,
      frequencyDelay: true,
      sevenBand: true,
      morphEq: true,
      filterBank: true,
      xyPad: true,
      xyzPad: true,
      panLfo: true,
      envelopeFollower: true,
      noteEnvelope: true,
      lushSpace: true,
      tuner: true,
      stageStack: true,
      controlSurface: true,
      sendTap: true,
    }
    const instruments: Record<InstrumentKind, true> = {
      subtractiveSynth: true,
      fourOp: true,
      matrixFm: true,
      ringHybrid: true,
      harmonicStack: true,
      partialMorph: true,
      inharmonic: true,
      resynth: true,
      seedPatch: true,
      scanSynth: true,
      pluck: true,
      fingerBass: true,
      acousticString: true,
      membrane: true,
      drumRack: true,
      kick: true,
      drumVoice: true,
      sliceMap: true,
      sliceDeck: true,
      grainCloud: true,
      waveRide: true,
      acidLine: true,
      tripleOsc: true,
      waveLane: true,
      macroVoice: true,
      speechVoice: true,
      midiOut: true,
      vintagePoly: true,
      dualPoly: true,
      compactSynth: true,
      vintageSaw: true,
    }
    expect([...EFFECT_KINDS].sort()).toEqual(Object.keys(effects).sort())
    expect([...INSTRUMENT_KINDS].sort()).toEqual(
      Object.keys(instruments).sort()
    )
    expect(instrumentDescriptor("subtractiveSynth").name).toBe(
      "Subtractive synth"
    )
    expect(instrumentDescriptor("subtractiveSynth").params).toHaveLength(55)
    expect(effectDescriptor("compressor").defaults.type).toBe("compressor")
  })

  it.each(ALL)("%s: paramIndex is the place in the table", (_, descriptor) => {
    descriptor.params.forEach((info, index) => {
      expect(paramIndex(descriptor, info.id)).toBe(index)
      expect(paramInfo(descriptor, info.id)).toBe(info)
    })
    const ids = descriptor.params.map((info) => info.id)
    expect(new Set(ids).size).toBe(ids.length)
  })

  it("fails loudly on an id the table does not have", () => {
    const synth = instrumentDescriptor("subtractiveSynth")
    expect(() => paramIndex(synth, "filter.cutoff")).toThrow(
      'Subtractive synth has no parameter "filter.cutoff"'
    )
    expect(() => paramInfo(effectDescriptor("eq"), "peak4.q")).toThrow(
      'Parametric EQ has no parameter "peak4.q"'
    )
  })

  it("agrees with the document about which index is which setting", () => {
    // The real document sets a setting by index; the UI finds it by id.
    const synth = instrumentDescriptor("subtractiveSynth")
    const project = buildProject(emptyProject(), (run) => {
      const [channel] = run({
        type: "addChannel",
        instrument: "subtractiveSynth",
      })
      synth.params.forEach((info, param) => {
        const value = info.default === info.max ? info.min : info.max
        run({ type: "setInstrumentParam", channel, param, value })
      })
    })
    const source = project.channels[0].source
    if (source.type !== "instrument") throw new Error("not an instrument")
    for (const info of synth.params) {
      const expected = info.default === info.max ? info.min : info.max
      expect(readParam(source.params, info), info.id).toBeCloseTo(expected, 4)
    }
  })
})

describe("readParam and writeParam", () => {
  it.each(ALL)(
    "%s: every default reads as its descriptor's default",
    (_, descriptor) => {
      for (const info of descriptor.params) {
        expect(readParam(descriptor.defaults, info), info.id).toBe(info.default)
      }
    }
  )

  it.each(ALL)(
    "%s: every setting round-trips across its range",
    (_, descriptor) => {
      for (const info of descriptor.params) {
        for (const value of samples(info)) {
          const written = writeParam(descriptor.defaults, info, value)
          expect(readParam(written, info), `${info.id} = ${value}`).toBe(value)
          // Nothing else moved.
          for (const other of descriptor.params) {
            if (other.id === info.id) continue
            expect(readParam(written, other)).toBe(other.default)
          }
        }
      }
    }
  )

  it("stores a toggle as a boolean and a choice as its JSON string", () => {
    const compressor = effectDescriptor("compressor")
    const makeup = paramInfo(compressor, "autoMakeup")
    const detector = paramInfo(compressor, "detector")
    const on = writeParam(compressor.defaults, makeup, 1)
    expect(on.autoMakeup).toBe(true)
    expect(writeParam(on, makeup, 0).autoMakeup).toBe(false)
    expect(writeParam(on, detector, 1).detector).toBe("rms")
    expect(readParam({ ...on, detector: "rms" }, detector)).toBe(1)
  })

  it("writes into lists and nested objects without touching the original", () => {
    const synth = instrumentDescriptor("subtractiveSynth")
    const level = paramInfo(synth, "oscillators.1.level")
    const waveform = paramInfo(synth, "oscillators.2.waveform")
    const before = JSON.stringify(synth.defaults)

    const next = writeParam(synth.defaults, level, 0.5)
    expect(next.oscillators[1].level).toBe(0.5)
    expect(Array.isArray(next.oscillators)).toBe(true)
    expect(next.oscillators).toHaveLength(3)
    expect(JSON.stringify(synth.defaults)).toBe(before)
    // What the change did not touch is shared.
    expect(next.oscillators[0]).toBe(synth.defaults.oscillators[0])
    expect(next.filter).toBe(synth.defaults.filter)
    expect(next.type).toBe("subtractiveSynth")

    expect(writeParam(next, waveform, 4).oscillators[2].waveform).toBe("pulse")
  })

  it("brings a value into the setting's range, as the core does", () => {
    const synth = instrumentDescriptor("subtractiveSynth")
    const cutoff = paramInfo(synth, "filter.cutoffHz")
    const coarse = paramInfo(synth, "oscillators.0.coarse")
    const mode = paramInfo(synth, "filter.mode")
    expect(writeParam(synth.defaults, cutoff, 5).filter.cutoffHz).toBe(20)
    expect(writeParam(synth.defaults, cutoff, 1e9).filter.cutoffHz).toBe(20000)
    expect(writeParam(synth.defaults, coarse, 6.6).oscillators[0].coarse).toBe(
      7
    )
    expect(writeParam(synth.defaults, mode, 9).filter.mode).toBe("highPass")
    expect(clampParam(cutoff, Number.NaN)).toBe(cutoff.default)
    expect(clampParam(paramInfo(synth, "polyphony"), 3.4)).toBe(3)
  })

  it("refuses settings of another kind", () => {
    const cutoff = paramInfo(
      instrumentDescriptor("subtractiveSynth"),
      "filter.cutoffHz"
    )
    const reverb = effectDescriptor("reverb").defaults
    expect(() => readParam(reverb, cutoff)).toThrow(
      'These settings have no "filter.cutoffHz" (Cutoff)'
    )
    expect(() => writeParam(reverb, cutoff, 100)).toThrow("filter.cutoffHz")
  })

  it("reads the generated table, with its floats spelled the short way", () => {
    const delay = effectDescriptor("delay")
    expect(delay.params.map((info) => info.id)).toEqual(
      tables.effects.delay.params.map((info) => info.id)
    )
    // In the file: 0.3499999940395355 and 0.949999988079071.
    expect(delay.defaults.feedback).toBe(0.35)
    expect(paramInfo(delay, "feedback").max).toBe(0.95)
    expect(paramInfo(delay, "feedback").default).toBe(0.35)
    expect(shortFloat(0.699999988079071)).toBe(0.7)
    expect(shortFloat(-0.44999998807907104)).toBe(-0.45)
    expect(shortFloat(0.009999999776482582)).toBe(0.01)
    expect(shortFloat(20000)).toBe(20000)
    expect(shortFloat(0)).toBe(0)
    expect(shortFloat(0.7071067690849304)).toBe(0.70710677)
  })

  it("has the defaults a new instrument and a new effect are stored with", () => {
    // Exactly, so settings can be compared with them and loaded from them.
    const project = buildProject(emptyProject(), (run) => {
      const [, track] = run({
        type: "addChannel",
        instrument: "subtractiveSynth",
      })
      let effectTrack = track
      for (const [index, kind] of EFFECT_KINDS.entries()) {
        // Mixer tracks hold at most ten effects.
        if (index > 0 && index % 10 === 0) {
          const [nextTrack] = run({ type: "addMixerTrack" })
          effectTrack = nextTrack
        }
        run({ type: "addEffect", track: effectTrack, kind })
      }
    })
    const source = project.channels[0].source
    if (source.type !== "instrument") throw new Error("not an instrument")
    expect(source.params).toEqual(
      instrumentDescriptor("subtractiveSynth").defaults
    )
    const effects = project.mixer.tracks.flatMap((track) => track.effects)
    expect(effects).toHaveLength(EFFECT_KINDS.length)
    for (const slot of effects) {
      expect(slot.params).toEqual(effectDescriptor(slot.params.type).defaults)
    }
  })
})
