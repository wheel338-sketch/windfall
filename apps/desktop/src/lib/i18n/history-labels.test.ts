import { afterEach, describe, expect, it } from "vitest"

import lowering from "../../../../../crates/windfall-project/src/lower.rs?raw"
import { groupRuns } from "@/features/history/history-list"

import { catalogs, historyLabel, historyLabelText, message, setLanguage } from "."

afterEach(() => setLanguage("en"))

/**
 * Capitalised literals of the command lowering that are not history labels:
 * names of factory content and one example in a comment.
 */
const NOT_LABELS = new Set([
  "Change Osc 1 level", "Kick Reverb", "Kick Reverb 2", "Kick Reverb mix", "Kick pan", "Kick pan 2",
  "Kick track volume", "Kick volume", "Sidechain gain",
])

/** Every literal the backend can return as the label of an edit. */
function backendLabels(): Set<string> {
  const body = lowering.split("#[cfg(test)]")[0]
  const labels = new Set<string>()
  for (const match of body.matchAll(/"((?:[^"\\]|\\.)*)"/g)) {
    const literal = match[1]
    if (!/^[A-Z][A-Za-z0-9 ,'’/&+().:-]*$/.test(literal) || literal.endsWith(".") || literal.length > 60) continue
    // A literal handed to an error, a check or a comparison is not a label.
    const statement = body.slice(Math.max(0, match.index - 90), match.index).split(";").at(-1) ?? ""
    if (/invalid\(|CommandError|not_found\(|Err\(|refus|reason|panic!|expect\(|assert|debug_assert|unreachable!|\.contains\(|== $|starts_with\(/.test(statement)) continue
    labels.add(literal)
  }
  return labels
}

describe("history labels follow the language", () => {
  it("has an entry in every language for every label the backend writes", () => {
    const labels = backendLabels()
    expect(labels.size).toBeGreaterThan(200)
    for (const known of ["Add channel", "Change tempo", "Move notes", "Rename project"]) expect(labels.has(known), known).toBe(true)
    const missing = [...labels].filter((label) => !NOT_LABELS.has(label))
      .flatMap((label) => Object.entries(catalogs).filter(([, catalog]) => !Object.hasOwn(catalog, label)).map(([language]) => `${language}: ${label}`))
    expect(missing.sort()).toEqual([])
    // The two labels it makes from a setting's name.
    expect(lowering).toContain('format!("Change {}", ')
    expect(lowering).toContain('format!("Change script {}", ')
  })

  it("keeps a key, and values only when there are any", () => {
    expect(historyLabel("Solo channel")).toBe("Solo channel")
    const kept = historyLabel("Fill every {every} steps", { every: 1234 })
    expect(kept).toBe('windfall-label:{"key":"Fill every {every} steps","values":{"every":1234}}')
    // Written plainly, as the labels made before this were, in each language.
    expect(historyLabelText(kept, "en")).toBe("Fill every 1234 steps")
    expect(historyLabelText(kept, "es")).toBe(message("Fill every {every} steps", { every: "1234" }, "es"))
    expect(historyLabelText(kept, "es")).not.toBe(historyLabelText(kept, "en"))
  })

  it("makes the words when the history is shown, whichever language the edit was made in", () => {
    // An edit made while the interface was Spanish keeps the same label as one made in English.
    setLanguage("es")
    const made = historyLabel("Solo channel")
    expect(made).toBe("Solo channel")
    expect(historyLabelText(made)).toBe(message("Solo channel", {}, "es"))
    expect(historyLabelText(made)).not.toBe("Solo channel")
    // What the backend wrote: plain English, and a name made from a setting.
    expect(historyLabelText("Change tempo")).toBe(message("Change tempo", {}, "es"))
    expect(historyLabelText("Change Cutoff")).toBe(message("Change {name}", { name: message("Cutoff", {}, "es") }, "es"))
    expect(historyLabelText("Change script Depth")).toBe(message("Change script {name}", { name: message("Depth", {}, "es") }, "es"))
    // A setting of somebody else's plugin keeps its own name.
    expect(historyLabelText("Change Vendor Knob 3")).toBe(message("Change {name}", { name: "Vendor Knob 3" }, "es"))
    setLanguage("en")
    expect(historyLabelText(made)).toBe("Solo channel")
    expect(historyLabelText("Change Cutoff")).toBe("Change Cutoff")
  })

  it("shows what it does not know as it is", () => {
    setLanguage("es")
    // A label from an older version, a name somebody typed, and broken keeping.
    for (const label of ["Mi propio nombre", "Riff 7 (take 2)", 'windfall-label:{"key":"No such key","values":{}}', "windfall-label:{", 'windfall-label:{"key":"Solo channel"}'])
      expect(historyLabelText(label)).toBe(label)
    // A coded backend message is decoded like anywhere else.
    expect(historyLabelText('windfall-message:{"code":"midi.import.markersNotImported","params":{"count":2}}'))
      .toBe("No se han importado 2 marcadores")
  })

  it("groups the steps of the history by the words shown", () => {
    setLanguage("es")
    const entries = [{ label: "Solo channel" }, { label: "Solo channel" }, { label: historyLabel("Fill every {every} steps", { every: 4 }) }]
    const runs = groupRuns(entries)
    expect(runs).toEqual([
      { label: message("Solo channel", {}, "es"), first: 1, last: 2 },
      { label: message("Fill every {every} steps", { every: "4" }, "es"), first: 3, last: 3 },
    ])
    setLanguage("en")
    expect(groupRuns(entries).map((run) => run.label)).toEqual(["Solo channel", "Fill every 4 steps"])
  })
})
