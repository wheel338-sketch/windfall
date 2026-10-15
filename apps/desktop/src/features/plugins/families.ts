import type { EffectKind, InstrumentKind } from "@/bindings"
import { EFFECT_KINDS, INSTRUMENT_KINDS } from "@/features/params/descriptors"
import { translatedText as t } from "@/lib/i18n"

/**
 * A family of built-in instruments or effects: what the menus that add one
 * group it under, and a line on what the family is for, so a long list can
 * be surveyed by role before by name.
 */
export type Family<Kind extends string> = {
  /** Stable, for keys and tests. */
  readonly id: string
  readonly label: string
  /** What the family sounds like or is used for, in a few words. */
  readonly role: string
  readonly kinds: readonly Kind[]
}

type FamilyTable<Kind extends string> = readonly {
  id: string
  label: () => string
  role: () => string
  kinds: readonly Kind[]
}[]

// The labels are the categories the plugin database files a built-in under
// where the two agree ("Synths", "Drums", "Dynamics" and so on), so the
// menus and the plugin browser speak of the same groups.
const INSTRUMENTS: FamilyTable<InstrumentKind> = [
  {
    id: "synths",
    label: () => t("Synths"),
    role: () => t("Leads, pads and chords from oscillators"),
    kinds: [
      "compactSynth",
      "subtractiveSynth",
      "tripleOsc",
      "vintagePoly",
      "dualPoly",
      "vintageSaw",
      "pocketSynth",
      "grooveSynth",
      "packSynth",
      "macroVoice",
      "waveLane",
      "fourOp",
      "matrixFm",
      "matrixSynth",
      "ringHybrid",
      "hybridSeq",
    ],
  },
  {
    id: "bass",
    label: () => t("Bass"),
    role: () => t("Bass lines and low end"),
    kinds: ["acidLine", "fingerBass"],
  },
  {
    id: "keys",
    label: () => t("Keys and strings"),
    role: () => t("Piano, plucked and bowed tones"),
    kinds: ["pianoKeys", "pluck", "acousticString"],
  },
  {
    id: "drums",
    label: () => t("Drums"),
    role: () => t("Kicks, kits and percussion"),
    kinds: ["drumRack", "kick", "layerKick", "drumVoice", "membrane"],
  },
  {
    id: "samplers",
    label: () => t("Samplers"),
    role: () => t("Play your own audio: samples, slices, SoundFonts and grains"),
    kinds: [
      "samplePlayer",
      "multiSampler",
      "soundFont",
      "padBank",
      "sliceMap",
      "sliceDeck",
      "grainCloud",
      "waveRide",
    ],
  },
  {
    id: "additive",
    label: () => t("Additive and spectral"),
    role: () => t("Tones built from partials: bells, glassy and evolving timbres"),
    kinds: [
      "harmonicStack",
      "partialMorph",
      "snapshotMorph",
      "inharmonic",
      "metalVoice",
      "resynth",
      "spectralResynth",
      "harmonicFilter",
      "scanSynth",
      "colorScan",
      "seedPatch",
      "numberVoice",
    ],
  },
  {
    id: "voices",
    label: () => t("Voices"),
    role: () => t("Synthetic speech and spoken text"),
    kinds: ["speechVoice", "textVoice"],
  },
  {
    id: "utility",
    label: () => t("Utility"),
    role: () => t("Drive outside gear and show controls"),
    kinds: ["midiOut", "dashboard"],
  },
]

const EFFECTS: FamilyTable<EffectKind> = [
  {
    id: "dynamics",
    label: () => t("Dynamics"),
    role: () => t("Control level and punch: compressors, limiters, gates"),
    kinds: [
      "compressor",
      "limiter",
      "masterLimiter",
      "multibandCompressor",
      "multibandMaximizer",
      "oneKnob",
      "stageStack",
      "transientShaper",
      "transientSplit",
      "volumeGate",
    ],
  },
  {
    id: "eq",
    label: () => t("EQ and filters"),
    role: () => t("Shape the tone by frequency"),
    kinds: [
      "eq",
      "sevenBand",
      "morphEq",
      "bassShelf",
      "fastLowpass",
      "selectableFilter",
      "filterBank",
      "bandSplit",
    ],
  },
  {
    id: "space",
    label: () => t("Delay and reverb"),
    role: () => t("Echoes and space"),
    kinds: [
      "delay",
      "bandDelay",
      "echoBank",
      "frequencyDelay",
      "reverb",
      "room",
      "lushSpace",
      "convolver",
    ],
  },
  {
    id: "modulation",
    label: () => t("Modulation"),
    role: () => t("Movement: chorus, flanger, phaser, auto-pan"),
    kinds: [
      "chorus",
      "vintageChorus",
      "hyperChorus",
      "flanger",
      "stackedFlanger",
      "phaser",
      "vintagePhaser",
      "panLfo",
      "frequencyShifter",
    ],
  },
  {
    id: "distortion",
    label: () => t("Distortion"),
    role: () => t("Drive, saturation, amps and lo-fi grit"),
    kinds: [
      "distortion",
      "overdrive",
      "driveChain",
      "softClipper",
      "waveshaper",
      "exciter",
      "bassHarmonics",
      "guitarRack",
      "lofi",
    ],
  },
  {
    id: "pitch",
    label: () => t("Pitch and voice"),
    role: () => t("Retune, shift and vocode"),
    kinds: ["pitchCorrect", "pitchShift", "vocoder"],
  },
  {
    id: "performance",
    label: () => t("Performance"),
    role: () => t("Live tricks on recent audio: scratch, slice, stutter"),
    kinds: ["scratch", "performanceRack", "timeTransport"],
  },
  {
    id: "utility",
    label: () => t("Utility"),
    role: () => t("Stereo, polarity, signal taps and tuning"),
    kinds: [
      "stereoEnhancer",
      "spreader",
      "stereoMatrix",
      "balance",
      "polarity",
      "dcBlock",
      "channelMute",
      "sendTap",
      "tuner",
    ],
  },
  {
    id: "control",
    label: () => t("Control"),
    role: () => t("Sources that move other settings: pads, envelopes, formulas"),
    kinds: [
      "xyPad",
      "xyzPad",
      "controlSurface",
      "envelopeFollower",
      "noteEnvelope",
      "keyboardSource",
      "formulaSource",
      "voltageSource",
      "musicLighting",
    ],
  },
]

/**
 * The families with their labels in the current language. Kinds a family
 * names that do not exist are left out, and kinds no family names go to an
 * "Other" family at the end, so a new instrument or effect is always
 * offered even before it is filed.
 */
function families<Kind extends string>(
  table: FamilyTable<Kind>,
  all: readonly Kind[]
): Family<Kind>[] {
  const known = new Set(all)
  const filed = new Set(table.flatMap((family) => family.kinds))
  const grouped = table
    .map((family) => ({
      id: family.id,
      label: family.label(),
      role: family.role(),
      kinds: family.kinds.filter((kind) => known.has(kind)),
    }))
    .filter((family) => family.kinds.length > 0)
  const rest = all.filter((kind) => !filed.has(kind))
  return rest.length === 0
    ? grouped
    : [...grouped, { id: "other", label: t("Other"), role: "", kinds: rest }]
}

/** The built-in instruments, by family. */
export function instrumentFamilies(): Family<InstrumentKind>[] {
  return families(INSTRUMENTS, INSTRUMENT_KINDS)
}

/** The built-in effects, by family. */
export function effectFamilies(): Family<EffectKind>[] {
  return families(EFFECTS, EFFECT_KINDS)
}

/**
 * What each built-in is for, by `builtinKey`: the role of its family, in
 * the current language. Kinds in no family have none.
 */
export function builtinRoles(): Map<string, string> {
  const roles = new Map<string, string>()
  for (const family of instrumentFamilies())
    for (const kind of family.kinds)
      if (family.role) roles.set(builtinKey(kind, true), family.role)
  for (const family of effectFamilies())
    for (const kind of family.kinds)
      if (family.role) roles.set(builtinKey(kind, false), family.role)
  return roles
}

/** Tells an instrument from an effect of the same kind name. */
export function builtinKey(kind: string, instrument: boolean): string {
  return `${instrument ? "instrument" : "effect"}:${kind}`
}
