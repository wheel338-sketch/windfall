import { afterAll, beforeAll, describe, expect, it } from "vitest"

import { registerAllActions } from "@/features/layout/register-actions"
import { registry } from "@/lib/actions"
import { BUILTIN_ACTIONS } from "@/lib/actions/builtin"

import { searchActions } from "./command-palette"
import { rank, scoreAction, shareOfWords } from "./score"

/** Titles of the built-in actions that match, best first. */
function search(text: string): string[] {
  return searchActions(BUILTIN_ACTIONS, text).map((action) => action.title)
}

describe("scoreAction", () => {
  it("matches everything for an empty search", () => {
    expect(search("  ")).toHaveLength(BUILTIN_ACTIONS.length)
  })

  it("puts the action whose title was typed first", () => {
    expect(search("add pattern")[0]).toBe("Add pattern")
    expect(search("mixer")[0]).toBe("Mixer")
    expect(search("save")[0]).toBe("Save")
    expect(search("undo")).toEqual(["Undo"])
  })

  it("ranks the title that is the search above titles that only start with it", () => {
    expect(search("play")[0]).toBe("Play")
    expect(search("play")).toContain("Play or stop")
    expect(scoreAction("Play", "", "play")).toBe(1)
    expect(scoreAction("Save as…", "", "save as")).toBe(1)
    expect(scoreAction("Play or stop", "", "play")).toBeLessThan(1)
    // The closer fit of two that start with it comes first.
    expect(scoreAction("Play song", "", "play")).toBeGreaterThan(
      scoreAction("Play the pattern from the start", "", "play")
    )
  })

  it("matches the starts of words in any order, the typed order first", () => {
    expect(search("pat add")[0]).toBe("Add pattern")
    expect(search("proj new")[0]).toBe("New project")
    expect(scoreAction("Add pattern", "", "add pat")).toBeGreaterThan(
      scoreAction("Add pattern", "", "pat add")
    )
  })

  it("ranks whole words of the title above a match inside a word, and that above keywords", () => {
    const start = scoreAction("Mute track", "", "mute")
    const inside = scoreAction("Unmute all tracks", "", "mute")
    const keyword = scoreAction("Silence", "mute quiet", "mute")
    expect(start).toBeGreaterThan(inside)
    expect(inside).toBeGreaterThan(keyword)
    expect(keyword).toBeGreaterThan(0)
  })

  it("ranks a title match above a keyword match", () => {
    expect(
      scoreAction("Export audio…", "render bounce wav", "exp")
    ).toBeGreaterThan(scoreAction("Mixer", "export", "exp"))
    // A search half in the title beats one all in the keywords.
    expect(scoreAction("Export audio…", "wav", "export wav")).toBeGreaterThan(
      scoreAction("Mixer", "export wav", "export wav")
    )
  })

  it("finds actions by keyword", () => {
    expect(search("bounce")).toEqual(["Export audio…"])
    expect(search("theme")).toContain("Switch between light and dark")
    expect(search("fl")).toContain("Use FL Studio shortcuts")
  })

  it("accepts letters that only appear in order, below every match on words", () => {
    expect(search("sv as")[0]).toBe("Save as…")
    expect(search("cmdpal")[0]).toBe("Command palette…")
    expect(scoreAction("Save as…", "", "svas")).toBeLessThan(
      scoreAction("Save as…", "", "save")
    )
    // "t all" is scattered over two words. A keyword is a better match.
    expect(scoreAction("Select all", "", "tall")).toBeLessThan(
      scoreAction("Row height", "tall", "tall")
    )
  })

  it("hides what does not match at all", () => {
    expect(search("zzzz")).toEqual([])
    expect(scoreAction("Play", "", "playx")).toBe(0)
  })
})

describe("the whole list of actions", () => {
  let unregister: () => void
  beforeAll(() => {
    unregister = registerAllActions()
  })
  afterAll(() => unregister())

  const titles = (text: string) =>
    searchActions(registry.list(), text).map((action) => action.title)

  it('puts "Tall tracks" first for "tall"', () => {
    const found = titles("tall")
    expect(found[0]).toBe("Tall tracks")
    // The scattered matches follow it.
    expect(found).toContain("Select all")
    expect(found.indexOf("Select all")).toBeGreaterThan(0)
  })

  it('puts "Tall tracks" first for "tall tracks", before "Unmute all tracks"', () => {
    const found = titles("tall tracks")
    expect(found[0]).toBe("Tall tracks")
    expect(found.indexOf("Unmute all tracks")).toBeGreaterThan(0)
  })

  it("finds the common ones by a word or two", () => {
    expect(titles("mixer")[0]).toBe("Mixer")
    expect(titles("playlist")[0]).toBe("Playlist")
    expect(titles("unmute all")[0]).toBe("Unmute all")
    expect(titles("unmute all").indexOf("Unmute all tracks")).toBeGreaterThan(0)
    expect(titles("delete track")[0]).toBe("Delete mixer track")
    expect(titles("new proj")[0]).toBe("New project")
    expect(titles("export audio")[0]).toBe("Export audio…")
    expect(titles("export midi")[0]).toBe("Export MIDI…")
    expect(titles("export")).toEqual(
      expect.arrayContaining(["Export audio…", "Export MIDI…"])
    )
    // Every result that starts with the word comes before any that only
    // has it inside.
    const mute = titles("mute")
    const firstInside = mute.findIndex(
      (title) => !title.toLowerCase().startsWith("mute")
    )
    expect(firstInside).toBeGreaterThan(0)
    expect(mute.slice(firstInside).some((title) => /^mute/i.test(title))).toBe(
      false
    )
  })
})

describe("rank", () => {
  it("keeps equal matches in the order they came in", () => {
    const items = ["Mute clips", "Mute track", "Unmute", "Mute notes"]
    expect(rank(items, "mute", (title) => ({ title, keywords: "" }))).toEqual([
      "Mute clips",
      "Mute track",
      "Mute notes",
      "Unmute",
    ])
  })
})

describe("shareOfWords", () => {
  it("counts the search words that start a title or keyword word, leaving out short ones", () => {
    expect(shareOfWords("Audio clip settings", "inspector editor", "audio editor")).toBe(1)
    expect(shareOfWords("Save", "write project", "save zzzz")).toBe(0.5)
    expect(shareOfWords("Save", "write project", "zzzz")).toBe(0)
    expect(shareOfWords("Save", "", "save to")).toBe(1)
    expect(shareOfWords("Save", "", "to")).toBe(0)
  })
})
