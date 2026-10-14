import { describe, expect, it } from "vitest"

import arrangement from "../../../../../crates/windfall-project/src/arrangement.rs?raw"
import check from "../../../../../crates/windfall-project/src/check.rs?raw"
import lowering from "../../../../../crates/windfall-project/src/lower.rs?raw"
import preset from "../../../../../crates/windfall-project/src/preset.rs?raw"
import audioEdit from "../../../../../crates/windfall-ipc/src/audio_edit.rs?raw"
import flpEffects from "../../../../../crates/windfall-flp/src/convert/effects.rs?raw"
import flpNative from "../../../../../crates/windfall-flp/src/convert/native.rs?raw"
import flpSynth from "../../../../../crates/windfall-flp/src/convert/synth.rs?raw"
import flpVoice from "../../../../../crates/windfall-flp/src/convert/voice.rs?raw"
import flpMod from "../../../../../crates/windfall-flp/src/convert/mod.rs?raw"
import flpNames from "../../../../../crates/windfall-flp/src/names.rs?raw"
import flpParse from "../../../../../crates/windfall-flp/src/parse.rs?raw"

import { catalogs } from "."

/** The source before its first test module. */
const product = (source: string) => source.split(/#\[cfg\(test\)\]\s*mod/)[0]

/** Every first capture of `pattern` in `source`. */
const captured = (source: string, pattern: RegExp) => [...product(source).matchAll(pattern)].map((match) => match[1])

/** The string literals of `source` between `from` and the next `to`. */
function literalsBetween(source: string, from: RegExp, to: string): string[] {
  const text = product(source)
  const start = text.search(from)
  if (start < 0) return []
  const end = text.indexOf(to, start)
  return [...text.slice(start, end).matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((match) => match[1])
}

/**
 * The words the import review shows as terms: every prose literal of the FL
 * conversion's effect, synth and native-layout modules, except a layout's
 * field names, a device name used as a key and identifier lists (the rule of
 * target/round8/flp_notes.py, which listed them for translation).
 */
function flpNotes(): string[] {
  const out = new Set<string>()
  for (const source of [flpEffects, flpSynth, flpNative]) {
    const body = product(source)
    for (const match of body.matchAll(/"((?:[^"\\]|\\.)*)"/g)) {
      const text = match[1].replaceAll('\\"', '"')
      if (!text.includes(" ") || (text.match(/[A-Za-z]{3,}/g) ?? []).length < 3 || text.includes("{")) continue
      const before = body.slice(Math.max(0, match.index - 40), match.index)
      const after = body.slice(match.index + match[0].length, match.index + match[0].length + 40).trimStart()
      if (/Layout::\w+\(\s*$/.test(before) || after.startsWith(".split_whitespace()")) continue
      if (after.startsWith("=>") || after.startsWith("|") || /(==|eq_ignore_ascii_case\()\s*$/.test(before)) continue
      if (text.toLowerCase().startsWith("fruity ") || (/^[a-z0-9_ ]+$/.test(text) && text.includes("_"))) continue
      out.add(text)
    }
  }
  return [...out]
}

/** Each family of terms: where it comes from, and the words. */
const FAMILIES: [string, string[]][] = [
  ["kinds an id belongs to (project validation, edits)", [
    ...captured(check, /check_id\(\s*\w+,\s*"([^"]+)"/g),
    ...captured(lowering, /not_found\(\s*"([^"]+)"/g),
    ...captured(arrangement, /kind: "([^"]+)"/g),
  ]],
  ["the settings an edit refuses a NaN for", captured(lowering, /clamped\(\s*"([^"]+)"/g)],
  ["the project fields with a length limit", literalsBetween(lowering, /for \(value, target, field, cap\) in \[/, "] {").filter((word) => /^[a-z]+$/.test(word))],
  ["envelope stages a project file is checked for", literalsBetween(check, /let times = \[/, "];")],
  ["what a preset is and what it holds", [
    ...literalsBetween(preset, /pub fn label\(self\) -> &'static str/, "}\n}"),
    ...captured(preset, /name_problem\(&[\w.]+, "([^"]+)"\)/g),
    ...captured(preset, /migrate_params\([^)]*?"(effect|instrument)"/g),
  ]],
  ["audio edit operations, named in history", literalsBetween(audioEdit, /pub fn label\(&self\) -> &'static str/, "\n    }\n}")],
  ["FL import notes and rules", flpNotes()],
  ["FL event names", captured(flpNames, /=> "([^"]+)"/g)],
  ["FL parts read only up to a limit", [...captured(flpParse, /self\.over\("([^"]+)"\)/g), ...captured(flpParse, /over_cap\.entry\("([^"]+)"\)/g)]],
  ["FL envelope and LFO targets and fields, arpeggiator and echo settings", [
    ...literalsBetween(flpVoice, /let target = match index \{/, "};"),
    ...captured(flpVoice, /\(\s*"([a-zA-Z -]+)",\s*(?:env|params|p)\./g),
    ...captured(flpVoice, /\(\d+, "([a-z -]+)"\)/g),
  ]],
  ["FL project fields with no place yet", captured(flpMod, /filled\(&settings\.\w+\), "([^"]+)"\)/g)],
]

describe("terms the backend sends inside coded messages", () => {
  it("has an entry for every one of them in every language", () => {
    const missing: string[] = []
    for (const [family, words] of FAMILIES) {
      expect(words.length, family).toBeGreaterThan(0)
      for (const word of words) for (const [language, catalog] of Object.entries(catalogs))
        if (!Object.hasOwn(catalog, word)) missing.push(`${language}: ${family}: ${word}`)
    }
    expect(missing).toEqual([])
  })

  it("finds the families where they are written", () => {
    const sizes = Object.fromEntries(FAMILIES.map(([family, words]) => [family, new Set(words).size]))
    expect(sizes["FL event names"]).toBeGreaterThan(140)
    expect(sizes["FL import notes and rules"]).toBeGreaterThan(45)
    expect(sizes["audio edit operations, named in history"]).toBeGreaterThan(20)
    expect(sizes["the settings an edit refuses a NaN for"]).toBeGreaterThan(15)
    expect(sizes["kinds an id belongs to (project validation, edits)"]).toBeGreaterThan(10)
  })
})
