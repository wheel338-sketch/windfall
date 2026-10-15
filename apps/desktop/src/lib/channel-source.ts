import type {
  Channel,
  ChannelSource,
  InstrumentParams,
  SampleId,
} from "@/bindings"

/*
 * A channel is a sampler or an instrument. These narrow one to the other,
 * so code that needs a sample or an instrument's settings says so once.
 */

export type SamplerSource = Extract<ChannelSource, { type: "sampler" }>
export type InstrumentSource = Extract<ChannelSource, { type: "instrument" }>

export type SamplerChannel = Channel & { source: SamplerSource }
export type InstrumentChannel = Channel & { source: InstrumentSource }

export function isSamplerChannel(channel: Channel): channel is SamplerChannel {
  return channel.source.type === "sampler"
}

export function isInstrumentChannel(
  channel: Channel
): channel is InstrumentChannel {
  return channel.source.type === "instrument"
}

/** The sample a sampler plays. Null for an instrument and for an empty sampler. */
export function sourceSample(
  source: ChannelSource | undefined
): SampleId | null {
  return source?.type === "sampler" ? source.sample : null
}

/** The settings of the channel's instrument, or null for a sampler. */
export function instrumentParams(
  channel: Channel | undefined
): InstrumentParams | null {
  return channel?.source.type === "instrument" ? channel.source.params : null
}

/** Instruments that play a sample-based granular source. */
const PLAYBACK_KINDS = new Set(["sliceMap", "sliceDeck", "grainCloud", "waveRide"])
/**
 * Instruments that play from a source they do not make themselves: a
 * SoundFont, mapped samples, a rendered phrase or a sample. Each is silent
 * until it has one. The MCP server's `source_needed` lists the same kinds.
 */
export const SOURCE_KINDS = new Set([
  "soundFont",
  "multiSampler",
  "samplePlayer",
  "padBank",
  "textVoice",
  ...PLAYBACK_KINDS,
])

/**
 * What a silent channel lacks: "sample" for a sampler with none, "soundFont"
 * for a SoundFont player with none, "source" for another sample-based
 * instrument with none. Null when the channel makes its own sound or has
 * its source. The rack marks these "no sample".
 */
export function missingSource(channel: Channel): "sample" | "soundFont" | "source" | null {
  if (channel.source.type === "sampler") return channel.source.sample === null ? "sample" : null
  const kind = channel.source.params.type
  if (!SOURCE_KINDS.has(kind)) return null
  if (PLAYBACK_KINDS.has(kind)) {
    const playback = channel.samplePlayback
    return playback && (playback.sample !== null || playback.inlineSource) ? null : "source"
  }
  const asset = channel.sampleInstrument
  const has = asset !== undefined && asset !== null && (asset.type !== "zones" || asset.zones.length > 0)
  if (has) return null
  return kind === "soundFont" ? "soundFont" : "source"
}
