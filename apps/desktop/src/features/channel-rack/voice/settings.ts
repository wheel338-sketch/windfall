/** Persistent types generated from windfall-project. */
import type {
  ChannelId,
  ChannelTargetLfos,
  EchoTime,
  LayerSettings,
  ModulationEnvelope,
  ChannelVoiceSettings,
  TargetLfo,
  VoiceFilter,
} from "@/bindings"
export type {
  ChannelTargetLfos,
  LayerSettings,
  TargetLfo,
  VoiceFilter,
  VoiceFilterMode,
  ChannelNoteDivision as NoteDivision,
  ArpeggiatorMode,
  EchoTime,
  ChannelLfoShape as LfoShape,
  LfoTarget,
  ArpeggiatorSettings,
  NoteEchoSettings,
  PolyphonySettings,
  ModulationEnvelope,
  ChannelLfoSettings,
  ChannelEnvelopes,
  ChannelVoiceSettings,
} from "@/bindings"

/** The most channels one layer plays, as in windfall-project. */
export const MAX_LAYER_CHILDREN = 16
export const MIN_VOICE_CUTOFF_HZ = 20
export const MAX_VOICE_CUTOFF_HZ = 20_000

/** The sampler modulation targets that have an LFO of their own. */
export const LFO_TARGETS = [
  "volume",
  "pan",
  "pitch",
  "cutoff",
  "resonance",
] as const
export type TargetLfoName = (typeof LFO_TARGETS)[number]

const defaultEnvelope = (): ModulationEnvelope => ({
  enabled: false,
  attackMs: 2,
  decayMs: 200,
  sustain: 0.8,
  releaseMs: 150,
  depth: 0,
})

export function createDefaultTargetLfo(): TargetLfo {
  return {
    enabled: false,
    shape: "sine",
    rateHz: 5,
    depth: 0,
    delayMs: 0,
    retrigger: false,
  }
}

export function createDefaultTargetLfos(): ChannelTargetLfos {
  return {
    volume: createDefaultTargetLfo(),
    pan: createDefaultTargetLfo(),
    pitch: createDefaultTargetLfo(),
    cutoff: createDefaultTargetLfo(),
    resonance: createDefaultTargetLfo(),
  }
}

export function createDefaultVoiceFilter(): VoiceFilter {
  return { enabled: false, mode: "lowPass", cutoffHz: 2000, resonance: 0 }
}

/**
 * The settings below are left out of a project while they are at their
 * defaults, so older files keep their shape. These read them with the
 * defaults filled in.
 */
export function resonanceEnvelopeOf(
  settings: ChannelVoiceSettings
): ModulationEnvelope {
  return settings.envelopes.resonance ?? defaultEnvelope()
}

export function targetLfosOf(
  settings: ChannelVoiceSettings
): ChannelTargetLfos {
  return settings.envelopes.lfos ?? createDefaultTargetLfos()
}

export function voiceFilterOf(settings: ChannelVoiceSettings): VoiceFilter {
  return settings.envelopes.filterSettings ?? createDefaultVoiceFilter()
}

export function layerOf(
  settings: ChannelVoiceSettings | undefined
): LayerSettings {
  return settings?.layer ?? { enabled: false, children: [] }
}

export function createDefaultChannelVoiceSettings(): ChannelVoiceSettings {
  return {
    arpeggiator: {
      mode: "off",
      rate: "sixteenth",
      gate: 0.75,
      rangeOctaves: 1,
    },
    echo: {
      enabled: false,
      time: { unit: "milliseconds", ms: 250 },
      feedback: 0.5,
      pitchSemitones: 0,
      repeats: 3,
    },
    polyphony: { maxVoices: 32, monoLegato: false, portamentoMs: 0 },
    envelopes: {
      filter: defaultEnvelope(),
      pitch: defaultEnvelope(),
      pan: defaultEnvelope(),
      lfo: {
        enabled: false,
        shape: "sine",
        target: "filter",
        rateHz: 5,
        depth: 0,
      },
    },
  }
}

function finite(value: number, low: number, high: number, fallback: number) {
  return Number.isFinite(value)
    ? Math.min(high, Math.max(low, value))
    : fallback
}

export function assertNever(value: never): never {
  throw new Error(`Unexpected channel voice value: ${String(value)}`)
}

/** Match Rust ranges and ensure integer fields can deserialize into u8/i8. */
export function sanitizeChannelVoiceSettings(
  value: ChannelVoiceSettings
): ChannelVoiceSettings {
  function envelope(
    value: ModulationEnvelope,
    depth: number
  ): ModulationEnvelope {
    return {
      ...value,
      attackMs: finite(value.attackMs, 0, 10_000, 2),
      decayMs: finite(value.decayMs, 1, 10_000, 200),
      sustain: finite(value.sustain, 0, 1, 0.8),
      releaseMs: finite(value.releaseMs, 1, 10_000, 150),
      depth: finite(value.depth, -depth, depth, 0),
    }
  }
  function targetLfo(value: TargetLfo): TargetLfo {
    return {
      ...value,
      rateHz: finite(value.rateHz, 0.01, 30, 5),
      depth: finite(value.depth, -1, 1, 0),
      delayMs: finite(value.delayMs ?? 0, 0, 20_000, 0),
      retrigger: value.retrigger === true,
    }
  }
  function layer(value: LayerSettings): LayerSettings {
    const children: ChannelId[] = []
    for (const child of value.children) {
      if (
        Number.isInteger(child) &&
        child >= 0 &&
        !children.includes(child) &&
        children.length < MAX_LAYER_CHILDREN
      )
        children.push(child)
    }
    return { enabled: value.enabled === true, children }
  }
  const { resonance, lfos, filterSettings } = value.envelopes
  function time(value: EchoTime): EchoTime {
    switch (value.unit) {
      case "milliseconds":
        return { unit: value.unit, ms: finite(value.ms, 1, 60_000, 250) }
      case "division":
        return { ...value }
      default:
        return assertNever(value)
    }
  }
  return {
    arpeggiator: {
      ...value.arpeggiator,
      gate: finite(value.arpeggiator.gate, 0, 1, 0.75),
      rangeOctaves: Math.trunc(finite(value.arpeggiator.rangeOctaves, 1, 4, 1)),
    },
    echo: {
      ...value.echo,
      time: time(value.echo.time),
      feedback: finite(value.echo.feedback, 0, 0.95, 0.5),
      pitchSemitones: Math.trunc(finite(value.echo.pitchSemitones, -48, 48, 0)),
      repeats: Math.trunc(finite(value.echo.repeats, 0, 8, 3)),
    },
    polyphony: {
      ...value.polyphony,
      maxVoices: Math.trunc(finite(value.polyphony.maxVoices, 1, 32, 32)),
      portamentoMs: finite(value.polyphony.portamentoMs, 0, 60_000, 0),
    },
    envelopes: {
      filter: envelope(value.envelopes.filter, 8),
      pitch: envelope(value.envelopes.pitch, 2400),
      pan: envelope(value.envelopes.pan, 1),
      lfo: {
        ...value.envelopes.lfo,
        rateHz: finite(value.envelopes.lfo.rateHz, 0.01, 30, 5),
        depth: finite(value.envelopes.lfo.depth, -1, 1, 0),
      },
      // Kept only when present, so untouched settings stay out of the edit.
      ...(resonance ? { resonance: envelope(resonance, 1) } : {}),
      ...(lfos
        ? {
            lfos: {
              volume: targetLfo(lfos.volume),
              pan: targetLfo(lfos.pan),
              pitch: targetLfo(lfos.pitch),
              cutoff: targetLfo(lfos.cutoff),
              resonance: targetLfo(lfos.resonance),
            },
          }
        : {}),
      ...(filterSettings
        ? {
            filterSettings: {
              ...filterSettings,
              cutoffHz: finite(
                filterSettings.cutoffHz,
                MIN_VOICE_CUTOFF_HZ,
                MAX_VOICE_CUTOFF_HZ,
                2000
              ),
              resonance: finite(filterSettings.resonance, 0, 1, 0),
            },
          }
        : {}),
    },
    ...(value.layer ? { layer: layer(value.layer) } : {}),
  }
}
