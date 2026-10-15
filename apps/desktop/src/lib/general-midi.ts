import type { SoundFontPreset } from "@/bindings"

/**
 * General MIDI's sixteen instrument families, eight programs each, in
 * program order. Bank 0 of a General MIDI SoundFont follows them; bank 128
 * holds its drum kits.
 */
export const GM_FAMILIES = [
  "Piano",
  "Chromatic Percussion",
  "Organ",
  "Guitar",
  "Bass",
  "Strings",
  "Ensemble",
  "Brass",
  "Reed",
  "Pipe",
  "Synth Lead",
  "Synth Pad",
  "Synth Effects",
  "Ethnic",
  "Percussive",
  "Sound Effects",
] as const

export type GmFamily = (typeof GM_FAMILIES)[number] | "Drum Kits" | "Other banks"

/** The 128 General MIDI program names, by program number. */
export const GM_PROGRAMS = [
  "Acoustic Grand Piano", "Bright Acoustic Piano", "Electric Grand Piano", "Honky-tonk Piano",
  "Electric Piano 1", "Electric Piano 2", "Harpsichord", "Clavinet",
  "Celesta", "Glockenspiel", "Music Box", "Vibraphone",
  "Marimba", "Xylophone", "Tubular Bells", "Dulcimer",
  "Drawbar Organ", "Percussive Organ", "Rock Organ", "Church Organ",
  "Reed Organ", "Accordion", "Harmonica", "Tango Accordion",
  "Acoustic Guitar (nylon)", "Acoustic Guitar (steel)", "Electric Guitar (jazz)", "Electric Guitar (clean)",
  "Electric Guitar (muted)", "Overdriven Guitar", "Distortion Guitar", "Guitar Harmonics",
  "Acoustic Bass", "Electric Bass (finger)", "Electric Bass (pick)", "Fretless Bass",
  "Slap Bass 1", "Slap Bass 2", "Synth Bass 1", "Synth Bass 2",
  "Violin", "Viola", "Cello", "Contrabass",
  "Tremolo Strings", "Pizzicato Strings", "Orchestral Harp", "Timpani",
  "String Ensemble 1", "String Ensemble 2", "Synth Strings 1", "Synth Strings 2",
  "Choir Aahs", "Voice Oohs", "Synth Voice", "Orchestra Hit",
  "Trumpet", "Trombone", "Tuba", "Muted Trumpet",
  "French Horn", "Brass Section", "Synth Brass 1", "Synth Brass 2",
  "Soprano Sax", "Alto Sax", "Tenor Sax", "Baritone Sax",
  "Oboe", "English Horn", "Bassoon", "Clarinet",
  "Piccolo", "Flute", "Recorder", "Pan Flute",
  "Blown Bottle", "Shakuhachi", "Whistle", "Ocarina",
  "Lead 1 (square)", "Lead 2 (sawtooth)", "Lead 3 (calliope)", "Lead 4 (chiff)",
  "Lead 5 (charang)", "Lead 6 (voice)", "Lead 7 (fifths)", "Lead 8 (bass + lead)",
  "Pad 1 (new age)", "Pad 2 (warm)", "Pad 3 (polysynth)", "Pad 4 (choir)",
  "Pad 5 (bowed)", "Pad 6 (metallic)", "Pad 7 (halo)", "Pad 8 (sweep)",
  "FX 1 (rain)", "FX 2 (soundtrack)", "FX 3 (crystal)", "FX 4 (atmosphere)",
  "FX 5 (brightness)", "FX 6 (goblins)", "FX 7 (echoes)", "FX 8 (sci-fi)",
  "Sitar", "Banjo", "Shamisen", "Koto",
  "Kalimba", "Bagpipe", "Fiddle", "Shanai",
  "Tinkle Bell", "Agogo", "Steel Drums", "Woodblock",
  "Taiko Drum", "Melodic Tom", "Synth Drum", "Reverse Cymbal",
  "Guitar Fret Noise", "Breath Noise", "Seashore", "Bird Tweet",
  "Telephone Ring", "Helicopter", "Applause", "Gunshot",
] as const

/** The family a preset is listed under: its General MIDI family in bank 0, drum kits in bank 128, the rest by bank. */
export function gmFamily(preset: Pick<SoundFontPreset, "bank" | "program">): GmFamily {
  if (preset.bank === 0 && preset.program < 128) return GM_FAMILIES[Math.floor(preset.program / 8)]
  if (preset.bank === 128) return "Drum Kits"
  return "Other banks"
}

/** Presets grouped by family, in General MIDI order, each group by bank then program. */
export function groupPresets(presets: readonly SoundFontPreset[]): { family: GmFamily; presets: SoundFontPreset[] }[] {
  const order: GmFamily[] = [...GM_FAMILIES, "Drum Kits", "Other banks"]
  const groups = new Map<GmFamily, SoundFontPreset[]>()
  for (const preset of presets) {
    const family = gmFamily(preset)
    groups.set(family, [...(groups.get(family) ?? []), preset])
  }
  return order
    .filter((family) => groups.has(family))
    .map((family) => ({
      family,
      presets: [...(groups.get(family) ?? [])].sort((a, b) => a.bank - b.bank || a.program - b.program),
    }))
}

/** Whether a preset matches a search by its name, its family or its General MIDI name. */
export function presetMatches(preset: SoundFontPreset, search: string, familyName: (family: GmFamily) => string = (family) => family): boolean {
  const wanted = search.trim().toLowerCase()
  if (!wanted) return true
  const family = gmFamily(preset)
  const gm = preset.bank === 0 && preset.program < 128 ? GM_PROGRAMS[preset.program] : ""
  return [preset.name, family, familyName(family), gm].some((text) => text.toLowerCase().includes(wanted))
}
