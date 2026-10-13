import {
  instrumentDescriptor,
  paramInfo,
  readParam,
  writeParam,
  type InstrumentParamsOf,
} from "@/features/params"
import { message, translatedText as t } from "@/lib/i18n"

export type SynthSettings = InstrumentParamsOf<"subtractiveSynth">

export type SynthPreset = {
  id: string
  name: string
  /** What it is good for, for the status bar. */
  description: string
  params: SynthSettings
}

/** A setting by id: a number, or the stored name of a choice. */
type Changes = Record<string, number | string>

const descriptor = instrumentDescriptor("subtractiveSynth")

/**
 * The default settings with a few of them changed. Keep the numbers short:
 * the core stores 32-bit floats and writes them back with as few digits as
 * identify them, so 0.85 reads back as 0.85 and a sound comes back from the
 * project exactly as it is written here.
 */
function sound(changes: Changes): SynthSettings {
  let params = descriptor.defaults
  for (const [id, value] of Object.entries(changes)) {
    const info = paramInfo(descriptor, id)
    let number: number
    if (typeof value === "string") {
      number = info.choices.findIndex((choice) => choice.value === value)
      if (number < 0) throw new Error(message("\"{id}\" has no option \"{value}\"", { id, value }))
    } else {
      number = value
    }
    params = writeParam(params, info, number)
  }
  return params
}

/**
 * Starting points for the subtractive synth. Each is the init sound with a
 * handful of settings moved, so reading one shows how the sound is made. A
 * browser for saved sounds comes later; these are all there is for now.
 *
 * Every sound sets its own `gain`, because what it adds to the init sound
 * (a second oscillator, unison, resonance, drive) changes how loud it is.
 * The gains are matched from renders of the real engine: one full-velocity
 * C5 of each sound peaks within 1 dB of -14 dBFS, so stepping through the
 * sounds does not jump in level, and a chord of four stays at or under
 * -6 dBFS. `presets.test.ts` has the measurements and holds the
 * gains to them. The bound in `peak-estimate.ts`, which takes the worst
 * case at every stage, still keeps a single note of every sound clear of
 * full scale.
 *
 * Init is the core's default settings, so its level is not set here.
 */
export const SYNTH_PRESETS: readonly SynthPreset[] = [
  {
    id: "init",
    name: "Init",
    get description() { return t("One saw oscillator with the filter open: a blank page") },
    params: descriptor.defaults,
  },
  {
    id: "softPad",
    name: "Soft pad",
    get description() { return t("Slow, wide and warm, for held chords") },
    params: sound({
      unisonVoices: 5,
      unisonDetuneCents: 16,
      unisonSpread: 0.85,
      "filter.slope": "db24",
      "filter.cutoffHz": 1400,
      "filter.resonance": 0.15,
      "filter.envelopeOctaves": 1.5,
      "ampEnvelope.attackMs": 600,
      "ampEnvelope.decayMs": 1500,
      "ampEnvelope.sustain": 0.85,
      "ampEnvelope.releaseMs": 1800,
      "filterEnvelope.attackMs": 900,
      "filterEnvelope.decayMs": 2500,
      "filterEnvelope.sustain": 0.6,
      "filterEnvelope.releaseMs": 1800,
      "lfos.0.rateHz": 0.3,
      "lfos.0.cutoffOctaves": 0.3,
      ampVelocity: 0.3,
      gain: 0.169,
    }),
  },
  {
    id: "pluck",
    name: "Pluck",
    get description() { return t("A short, bright note that closes fast, for arpeggios") },
    params: sound({
      "filter.slope": "db24",
      "filter.cutoffHz": 450,
      "filter.resonance": 0.25,
      "filter.keyTracking": 0.5,
      "filter.envelopeOctaves": 4.5,
      "filter.velocity": 0.4,
      "ampEnvelope.attackMs": 1,
      "ampEnvelope.decayMs": 350,
      "ampEnvelope.sustain": 0,
      "ampEnvelope.releaseMs": 250,
      "filterEnvelope.attackMs": 0,
      "filterEnvelope.decayMs": 220,
      "filterEnvelope.sustain": 0,
      "filterEnvelope.releaseMs": 220,
      gain: 0.182,
    }),
  },
  {
    id: "bass",
    name: "Bass",
    get description() { return t("One note at a time with a square an octave down and some grit") },
    params: sound({
      "oscillators.1.waveform": "square",
      "oscillators.1.level": 0.6,
      "oscillators.1.coarse": -12,
      "filter.slope": "db24",
      "filter.cutoffHz": 280,
      "filter.resonance": 0.25,
      "filter.envelopeOctaves": 2.5,
      "filter.drive": 0.3,
      "ampEnvelope.attackMs": 1,
      "ampEnvelope.decayMs": 300,
      "ampEnvelope.sustain": 0.7,
      "ampEnvelope.releaseMs": 80,
      "filterEnvelope.decayMs": 180,
      "filterEnvelope.sustain": 0.2,
      voiceMode: "mono",
      glideMs: 30,
      // The square, the drive and the resonance each add level.
      gain: 0.067,
    }),
  },
  {
    id: "lead",
    name: "Lead",
    get description() { return t("A sliding solo voice with a touch of vibrato") },
    params: sound({
      "oscillators.1.waveform": "pulse",
      "oscillators.1.level": 0.6,
      "oscillators.1.fineCents": 7,
      "oscillators.1.pulseWidth": 0.3,
      "filter.cutoffHz": 3500,
      "filter.resonance": 0.2,
      "filter.keyTracking": 0.6,
      "filter.envelopeOctaves": 1,
      "ampEnvelope.attackMs": 5,
      "ampEnvelope.decayMs": 300,
      "ampEnvelope.releaseMs": 200,
      "lfos.0.rateHz": 5.5,
      "lfos.0.pitchSemitones": 0.15,
      voiceMode: "legato",
      glideMs: 60,
      gain: 0.15,
    }),
  },
  {
    id: "keys",
    name: "Keys",
    get description() { return t("A struck note that fades while held and answers to touch") },
    params: sound({
      "oscillators.0.waveform": "triangle",
      "oscillators.1.waveform": "pulse",
      "oscillators.1.level": 0.35,
      "oscillators.1.coarse": 12,
      "oscillators.1.pulseWidth": 0.4,
      "filter.cutoffHz": 2200,
      "filter.keyTracking": 0.5,
      "filter.envelopeOctaves": 1.5,
      "filter.velocity": 0.5,
      "ampEnvelope.decayMs": 900,
      "ampEnvelope.sustain": 0.25,
      "ampEnvelope.releaseMs": 350,
      "filterEnvelope.decayMs": 500,
      "filterEnvelope.sustain": 0.2,
      ampVelocity: 0.9,
      gain: 0.185,
    }),
  },
  {
    id: "brass",
    name: "Brass",
    get description() { return t("Two saws that open up as each note swells") },
    params: sound({
      "oscillators.1.level": 0.8,
      "oscillators.1.fineCents": -6,
      "filter.slope": "db24",
      "filter.cutoffHz": 380,
      "filter.envelopeOctaves": 3,
      "filter.velocity": 0.5,
      "ampEnvelope.attackMs": 35,
      "ampEnvelope.decayMs": 300,
      "ampEnvelope.sustain": 0.9,
      "ampEnvelope.releaseMs": 180,
      "filterEnvelope.attackMs": 70,
      "filterEnvelope.decayMs": 400,
      "filterEnvelope.sustain": 0.65,
      "lfos.0.pitchSemitones": 0.08,
      // As loud as its chords allow: four notes of it reach -6 dBFS.
      gain: 0.112,
    }),
  },
  {
    id: "sub",
    name: "Sub",
    get description() { return t("A plain sine for the very bottom, one note at a time") },
    params: sound({
      "oscillators.0.waveform": "sine",
      "oscillators.1.waveform": "triangle",
      "oscillators.1.level": 0.2,
      "oscillators.1.coarse": 12,
      "filter.cutoffHz": 600,
      "ampEnvelope.attackMs": 4,
      "ampEnvelope.sustain": 1,
      "ampEnvelope.releaseMs": 120,
      voiceMode: "mono",
      glideMs: 40,
      ampVelocity: 0.3,
      gain: 0.218,
    }),
  },
]

export function synthPreset(id: string): SynthPreset | undefined {
  return SYNTH_PRESETS.find((preset) => preset.id === id)
}

function sameSound(a: SynthSettings, b: SynthSettings): boolean {
  return descriptor.params.every(
    (info) => readParam(a, info) === readParam(b, info)
  )
}

const matched = new WeakMap<object, SynthPreset | null>()

/** The built-in sound these settings are, exactly, or null once edited. */
export function matchingPreset(params: SynthSettings): SynthPreset | null {
  let found = matched.get(params)
  if (found === undefined) {
    found =
      SYNTH_PRESETS.find((preset) => sameSound(preset.params, params)) ?? null
    matched.set(params, found)
  }
  return found
}
