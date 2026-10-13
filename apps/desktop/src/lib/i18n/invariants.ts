/**
 * The only exceptions to localization admission. Keep this data separate from
 * the scanner so a change to an exception is visible in code review. Authored
 * project/plugin/script text is not a source literal and needs no exception.
 */
export const invariantLiterals: Readonly<Record<string, string>> = {
  Windfall: "Product name.",
  MIDI: "Protocol acronym.",
  MPE: "Protocol acronym.",
  CLAP: "Plugin format identifier.",
  VST3: "Plugin format identifier.",
  AU: "Plugin format identifier.",
  SF2: "File format identifier.",
  WAV: "File format identifier.",
  FLAC: "File format identifier.",
  MP3: "File format identifier.",
  OGG: "File format identifier.",
  AIFF: "File format identifier.",
  WavPack: "File format name.",
  Opus: "Codec name.",
  PPQ: "Timing resolution acronym.",
  BPM: "Tempo unit acronym.",
  bpm: "Tempo unit acronym.",
  Hz: "SI frequency unit.",
  kHz: "SI frequency unit.",
  dB: "Decibel unit.",
  ms: "SI time unit.",
  s: "SI time unit.",
  st: "Semitone unit abbreviation.",
  ct: "Cent unit abbreviation.",
  oct: "Octave unit abbreviation shared by the shipped languages.",
  LFO: "Synthesis acronym.",
  FM: "Synthesis acronym.",
  RMS: "Measurement acronym.",
  Q: "Filter quality-factor symbol.",
  X: "Axis identifier.",
  Y: "Axis identifier.",
  Z: "Axis identifier.",
}

/** Scoped to keycaps: Space is also a translatable audio parameter name. */
export const invariantKeyboardKeys: Readonly<Record<string, string>> = {
  Shift: "Keyboard key name in a keycap.", Ctrl: "Keyboard key name in a keycap.",
  Alt: "Keyboard key name in a keycap.", Cmd: "Keyboard key name in a keycap.",
  Tab: "Keyboard key name in a keycap.", Escape: "Keyboard key name in a keycap.",
  Enter: "Keyboard key name in a keycap.", Space: "Keyboard key name in a keycap.",
}

export const invariantPatterns = [
  { pattern: /^[\s\d.,:;/%+−–—×÷∞°·→←♭♯#()[\]…!?=|<>∅★●○◀▶■□✓✕×-]*$/u, reason: "Numbers and notation symbols with no language-bearing words." },
  { pattern: /^[A-G](?:#|b|♯|♭)?-?\d*$/, reason: "International letter note names used by both shipped catalogs." },
  { pattern: /^\.(?:wav|aiff?|flac|ogg|opus|mp3|wv|mid|midi|sf2|windfall|wfp|zip)$/i, reason: "File extensions; never translated." },
  { pattern: /^(?:wav|aiff?|flac|ogg|opus|mp3|wv|mid|midi|sf2|zip)$/i, reason: "Technical codec/extension identifiers used as message parameters." },
  { pattern: /^(?:[+−-]?\d+(?:[.,]\d+)?|[−-]?∞)\s*(?:dB(?:\/oct)?|Hz|kHz|ms|s|st|ct|oct|%)$/, reason: "Measured numeric readouts with invariant units, including filter slopes." },
  { pattern: /^\d+x$/, reason: "Oversampling multiplier notation; x is a symbol in both languages." },
] as const

export function invariantReason(text: string, context?: string): string | undefined {
  return invariantLiterals[text] ?? (context === "keyboard shortcut" ? invariantKeyboardKeys[text] : undefined) ?? invariantPatterns.find(({ pattern }) => pattern.test(text))?.reason
}

/** Identical translations also require a reason, even for punctuation templates. */
export const sameAsEnglish: Readonly<Record<string, string>> = {
  "{name}: {value}": "Only parameters and punctuation; the name is localized at its source.",
  "{name}: {value}. {description}": "Only parameters and punctuation; semantic parameters are localized at their source.",
  "{name}{mark} - Windfall": "Authored project name, unsaved mark and product name.",
  "{panel} - Windfall": "Localized panel parameter and product name.",
  Audio: "Spanish uses the same noun for audio.",
  Mono: "Shared musical abbreviation for monophonic/mono.",
  mono: "Shared channel-layout abbreviation.",
  Variable: "Spanish uses the same adjective for variable-rate encoding.",
  Vibrato: "Established musical term in both languages.",
  "0": "Numeric endpoint.",
  "{help} ({shortcut})": "Only parameters and punctuation; both parameters are localized at their source.",
  "{value} PPQ": "Numeric parameter and invariant timing unit.",
  "{title} ({range})": "Only parameters and punctuation; semantic parameters are localized at their source.",
  "{label} · plugin": "Spanish music software uses plugin; the label parameter is localized at its source.",
}
