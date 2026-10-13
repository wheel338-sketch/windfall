import { describe, expect, it } from "vitest"
import palette from "@/features/palette/command-palette.tsx?raw"
import tutorial from "@/features/help/guided-tutorial.tsx?raw"
import nativeDialogs from "@/lib/ipc/tauri.ts?raw"
import builtin from "@/lib/actions/builtin.ts?raw"
import { actionMessages } from "./action-messages"
import { actionTitle, catalogs, translatedText } from "."
import type { MessageKey } from "./types"

describe("migrated shell localization boundaries", () => {
  it("every covered UI and native dialog literal has a key in every shipped language", () => {
    const keys = new Set<string>()
    for (const source of [palette, tutorial, nativeDialogs]) {
      for (const match of source.matchAll(/\b(?:t|translatedText)\("((?:\\.|[^"\\])*)"\)/g))
        keys.add(JSON.parse(`"${match[1]}"`) as string)
    }
    expect(keys.size).toBeGreaterThan(25)
    for (const key of keys) for (const catalog of Object.values(catalogs)) {
      expect(Object.hasOwn(catalog, key), `missing key: ${key}`).toBe(true)
      expect(catalog[key as MessageKey]).toBeTruthy()
    }
    // Covered components must never introduce raw JSX captions or unwrapped
    // display attributes. IDs, user text and IPC command names are excluded.
    for (const source of [palette, tutorial]) {
      const withoutComments = source.replace(/\{\/\*[\s\S]*?\*\/\}/g, "")
      expect(withoutComments.match(/>\s*[A-Za-z][^{}<>]*<\//g)).toBeNull()
      expect(withoutComments.match(/\b(?:title|description|placeholder|aria-label)="[A-Za-z][^"]*"/g)).toBeNull()
    }
    expect(nativeDialogs.match(/\b(?:title|name): "[A-Za-z][^"]*"/g)).toBeNull()
  })
  it("all static builtin command names are translated and recent project names remain authored text", () => {
    const names = [...builtin.matchAll(/id: "([^"]+)",\s*\.\.\.dynamicTitle\("([^"]+)", \{\}\)/g)]
    expect(names.length).toBeGreaterThan(30)
    for (const [, id, title] of names) {
      expect(actionMessages[id], `missing action key: ${id}`).toBeDefined()
      const translated = actionTitle(id, title, "es")
      expect(translated).not.toBe(title)
      expect(translated).toBe(translatedText(title, "es"))
    }
    expect(actionTitle("file.openRecent.0", "Save", "es")).toBe("Save")
  })
})
