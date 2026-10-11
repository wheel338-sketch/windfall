import type {
  MidiExportOptions,
  MidiImportOptions,
  PatternId,
  PlayMode,
} from "@/bindings"

export const DEFAULT_IMPORT: MidiImportOptions = {
  bars: 0,
  sharePatterns: true,
  factoryDrums: false,
  tempo: true,
  timeSignature: true,
  channelMix: true,
  noteExpression: true,
}

export function midiExportOptions(
  mode: PlayMode,
  pattern: PatternId
): MidiExportOptions {
  return {
    mode,
    pattern,
    singleTrack: false,
    ppq: 960,
    runningStatus: true,
    swing: true,
  }
}
