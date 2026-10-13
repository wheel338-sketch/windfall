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
  M4A: "File format identifier.",
  AAC: "Codec name.",
  WavPack: "File format name.",
  Opus: "Codec name.",
  SHA256: "Hash algorithm name.",
  "SHA-256": "Hash algorithm name.",
  PPQ: "Timing resolution acronym.",
  BPM: "Tempo unit acronym.",
  bpm: "Tempo unit acronym.",
  Hz: "SI frequency unit.",
  kHz: "SI frequency unit.",
  dB: "Decibel unit.",
  dBFS: "Full-scale level unit.",
  dBTP: "True-peak level unit.",
  LUFS: "Loudness unit.",
  kbps: "Bitrate unit.",
  ms: "SI time unit.",
  s: "SI time unit.",
  min: "Minute abbreviation shared by the shipped languages.",
  st: "Semitone unit abbreviation.",
  ct: "Cent unit abbreviation.",
  oct: "Octave unit abbreviation shared by the shipped languages.",
  ppm: "Parts-per-million unit.",
  px: "CSS pixel unit.",
  MB: "Data size unit.",
  MiB: "Binary data size unit.",
  GiB: "Binary data size unit.",
  k: "SI kilo suffix on a frequency tick.",
  LFO: "Synthesis acronym.",
  FM: "Synthesis acronym.",
  RMS: "Measurement acronym.",
  DC: "Zero-frequency signal acronym.",
  CC: "MIDI control-change acronym.",
  FX: "Effects abbreviation that Spanish music software uses unchanged.",
  RAM: "Hardware acronym.",
  CPU: "Hardware acronym.",
  Q: "Filter quality-factor symbol.",
  X: "Axis identifier.",
  Y: "Axis identifier.",
  Z: "Axis identifier.",
  x: "Axis or formula variable.",
  y: "Axis or formula variable.",
  a: "Formula variable in the controller formula syntax.",
  b: "Formula variable in the controller formula syntax.",
  c: "Formula variable in the controller formula syntax.",
  L: "Left channel letter.",
  R: "Right channel letter.",
  M: "Mute lamp letter that Spanish music software uses unchanged.",
  S: "Solo lamp letter that Spanish music software uses unchanged.",
  W: "Product logo initial.",
  Op: "Operator abbreviation shared by the shipped languages.",
  Osc: "Oscillator abbreviation shared by the shipped languages.",
  "FL Studio": "Third-party product name.",
  Nyquist: "Proper name of the Nyquist frequency.",
  Moss: "Name of the built-in visualizer character.",
  English: "Language endonym, listed under its own name in every locale.",
  Español: "Language endonym, listed under its own name in every locale.",
  "Swarm Chorus": "Coined name of a built-in effect.",
  "Lush Space": "Coined name of a built-in effect.",
  "Stage Stack": "Coined name of a built-in effect.",
  "Bucket Chorus": "Coined name of a built-in effect.",
  "Stage Phaser": "Coined name of a built-in effect.",
  "One Knob": "Coined name of a built-in effect.",
  Ember: "Coined name of a factory sound pack.",
  Prism: "Coined name of a factory sound pack.",
  Meadow: "Coined name of a factory sound pack.",
  sm: "Component size token shown in the developer control kit.",
  md: "Component size token shown in the developer control kit.",
  lg: "Component size token shown in the developer control kit.",
}

/** Scoped to keycaps: Space is also a translatable audio parameter name. */
export const invariantKeyboardKeys: Readonly<Record<string, string>> = {
  Shift: "Keyboard key name in a keycap.", Ctrl: "Keyboard key name in a keycap.",
  Alt: "Keyboard key name in a keycap.", Cmd: "Keyboard key name in a keycap.",
  Tab: "Keyboard key name in a keycap.", Escape: "Keyboard key name in a keycap.",
  Enter: "Keyboard key name in a keycap.", Space: "Keyboard key name in a keycap.",
}

/** Notation that carries no words: digits, punctuation, arrows, musical and UI symbols. */
const SYMBOLS = String.raw`\s\d.,:;/%+−–—×÷∞°·→←↑↓♭♯#()[\]…!?=|<>∅★☆●○◀▶▸▾■□✓✕⋮⋯±¢*⌃⌥⇧⌘"-`

export const invariantPatterns: readonly (readonly [RegExp, string])[] = [
  [new RegExp(`^[${SYMBOLS}]*$`, "u"), "Numbers and notation symbols with no language-bearing words."],
  [/^[A-G](?:#|b|♯|♭)?-?\d*$/, "International letter note names used by both shipped catalogs."],
  [/^\.(?:wav|aiff?|flac|ogg|opus|mp3|wv|mid|midi|sf2|windfall|wfp|zip)$/i, "File extensions; never translated."],
  [/^(?:wav|aiff?|flac|ogg|opus|mp3|wv|mid|midi|sf2|zip)$/i, "Technical codec/extension identifiers used as message parameters."],
  [/^(?:[+−-]?\d+(?:[.,]\d+)?|[−-]?∞)\s*(?:dB(?:\/oct)?|dBFS|Hz|kHz|ms|s|st|ct|oct|px|%)$/, "Measured numeric readouts with invariant units, including filter slopes."],
  [/^\d+x$/, "Oversampling multiplier notation; x is a symbol in both languages."],
  [/^\d+\/\d+T$/, "Triplet note-value notation."],
  [/^(?:[IVX]+|[ivx]+)(?:\+[IVX]+)*$/, "Roman-numeral scale degrees and mode numbers."],
  [/^var\(--[\w-]+\)$/, "CSS custom property reference, not text."],
  [/^https?:\/\/[^\s]+$/, "URL; never translated."],
  [/^[a-z]{2}-[A-Z]{2}$/, "BCP 47 locale tag passed to Intl."],
]

const whole = (text: string, context?: string): string | undefined =>
  invariantLiterals[text] ?? (context === "keyboard shortcut" ? invariantKeyboardKeys[text] : undefined) ??
  invariantPatterns.find(([pattern]) => pattern.test(text))?.[1]

const edges = new RegExp(`^[${SYMBOLS}]+|[${SYMBOLS}]+$`, "gu")

export function invariantReason(text: string, context?: string): string | undefined {
  const reason = whole(text, context)
  if (reason) return reason
  // Invariant terms joined only by notation, such as "· kHz", "M4A (AAC)" or "C♯ / D♭".
  const terms = text.split(/\s+/).filter(Boolean)
  const invariant = (term: string) => !!whole(term, context) || !!whole(term.replace(edges, ""), context)
  return terms.length && terms.every(invariant) ? "Only invariant terms joined by notation symbols." : undefined
}

const each = (reason: string, keys: readonly string[]) => Object.fromEntries(keys.map((key) => [key, reason]))
const numbered = (word: string, count: number) => Array.from({ length: count }, (_, index) => `${word} ${index + 1}`)

/** Identical translations also require a reason, even for punctuation templates. */
export const sameAsEnglish: Readonly<Record<string, string>> = {
  ...each("Only parameters, punctuation and invariant names; each parameter is localized at its source.", [
    "r{revision}", "{scope} · {label}", "{device} · {control}", "{description}. {action}", "{slot}: {output}",
    "{device} · {channel} · {control}", "{owner} · {target}", "{owner} · {parameter}", "{device}: {left} / {right}",
    "{name}, {place}", "{root} {quality}", "{root} {quality} / {bass}", "{root} {scale}", "{id} ({name})",
    "{input} · {layout}", "{index}. {title}", "{name} · FL Studio", "Moss: {pose}", "Color {group} · MIDI {group2}",
  ]),
  ...each("Spanish music software counts in ticks, clips, cents, pads and bytes too; the rest is parameters and punctuation.", [
    "{value}{value2} cents", "{value} ticks", "{tick} ticks", " (ticks {start}–{end})", "Ticks {start}–{end}{active}",
    "{count} clips", "2400 cents", "Macro {index}", "Pad {index}, {pad}", "{device}: {left} mono", "Audio: {sourceName}",
    "{size}, bipolar", "{size}, unipolar", "bytes ·", "· tick", "(clip", ", tick", "· Solo", "ticks)", "ticks", "Tick",
    "Streaming, −14 LUFS", "Swing (%)",
  ]),
  ...each("Spanish spells this word the same way.", [
    "general", "General", "vocal", "Regular", "Balance", "Base", "Bipolar", "Color", "Detector", "Normal", "Pedal",
    "Nasal", "Triple", "Natural", "Control", "Tempo", "Solo", ...numbered("Pedal", 11),
  ]),
  ...each("Established term that Spanish-speaking producers use unchanged.", [
    "Plugins", "Mono legato", "Vol", "1x12 combo", "Combo", "Ducking", "Flanger", "Fuzz", "Notch", "Overdrive", "Post",
    "Pre", "Scratch", "Twin", "Vocoder", "Wah", "Legato", "Multisampler", "Tom", "Chorus", "Drive", "Phaser", "Stack",
    "2 Color", "Mod", "Scoop", "Fader", "Sampler", "Blues", "Drop 2", "Flam", "Groove", "Staccato", "Swing",
    "Portamento", "Pads", ...numbered("Pad", 16),
  ]),
  ...each("Keyboard key name printed the same on a Spanish keyboard.", ["Esc", "Num +", "Num -", "key.alt", "key.ctrl", "key.win"]),
  "Set tempo 120; create pattern Verse; add channel Bass": "Example in the offline assistant's command syntax, which is read in English.",
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
