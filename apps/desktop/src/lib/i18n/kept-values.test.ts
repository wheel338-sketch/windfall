import { afterEach, describe, expect, it } from "vitest"

import { PIANO_ROLL_ACTIONS } from "@/features/piano-roll/actions"
import { LANE_KINDS } from "@/features/piano-roll/lane-math"

import { dynamicMessage, dynamicTitle, inLanguage, message, setLanguage, translatedText as t } from "."

afterEach(() => setLanguage("en"))

describe("kept titles made of translated words", () => {
  it("keeps an English title for the registry and makes the word again in the language shown", () => {
    // Registered while the interface is Spanish, as after a restart in Spanish.
    setLanguage("es")
    const kept = dynamicTitle("Show {value0} under the notes", { value0: () => t("Velocity").toLowerCase() })
    expect(kept.title).toBe("Show velocity under the notes")
    const spanish = dynamicMessage(kept.titleMessage)
    expect(spanish).toBe(message("Show {value0} under the notes", { value0: t("Velocity").toLowerCase() }, "es"))
    expect(spanish).not.toContain("velocity")
    setLanguage("en")
    expect(dynamicMessage(kept.titleMessage)).toBe("Show velocity under the notes")
    // Asking for a language is independent of the interface language.
    expect(dynamicMessage(kept.titleMessage, "es")).toBe(spanish)
  })

  it("reads in another language without changing the interface language, and nests", () => {
    expect(inLanguage("es", () => t("Velocity"))).toBe(message("Velocity", {}, "es"))
    expect(t("Velocity")).toBe("Velocity")
    expect(inLanguage("es", () => inLanguage("en", () => t("Velocity")) + t("Velocity")))
      .toBe("Velocity" + message("Velocity", {}, "es"))
    expect(() => inLanguage("es", () => { throw new Error("stop") })).toThrow("stop")
    expect(t("Velocity")).toBe("Velocity")
  })

  it("piano-roll titles registered in one language follow a switch to the other", () => {
    // The action list is built once, at module load, in English here. Every
    // title that names a lane, a tool, a snap, a length or a colour must be
    // right in Spanish afterwards and right in English after switching back.
    for (const kind of LANE_KINDS) {
      const action = PIANO_ROLL_ACTIONS.find((item) => item.id === `pianoRoll.lane.${kind.id}`)!
      const english = `Show ${inLanguage("en", () => kind.label).toLowerCase()} under the notes`
      expect(action.title).toBe(english)
      setLanguage("es")
      const word = kind.label.toLowerCase()
      expect(dynamicMessage(action.titleMessage!)).toBe(message("Show {value0} under the notes", { value0: word }))
      setLanguage("en")
      expect(dynamicMessage(action.titleMessage!)).toBe(english)
    }
    const kept = PIANO_ROLL_ACTIONS.filter((action) =>
      Object.values(action.titleMessage?.values ?? {}).some((value) => typeof value === "function"))
    // Lanes, snaps, lengths, articulations, portamento presets, tools and the two colour actions per group.
    expect(kept.length).toBeGreaterThan(40)
    for (const action of kept) {
      const english = dynamicMessage(action.titleMessage!, "en")
      expect(action.title, action.id).toBe(english)
      setLanguage("es")
      const spanish = dynamicMessage(action.titleMessage!)
      setLanguage("en")
      // Made again from its parts in Spanish: the same as asking for Spanish outright.
      expect(spanish, action.id).toBe(dynamicMessage(action.titleMessage!, "es"))
      expect(dynamicMessage(action.titleMessage!), action.id).toBe(english)
    }
  })
})
