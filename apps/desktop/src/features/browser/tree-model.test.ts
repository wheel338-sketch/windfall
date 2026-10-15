import { describe, expect, it } from "vitest"

import type { BrowserEntry, BrowserRoot } from "@/bindings"

import {
  extensionStart,
  findByPrefix,
  firstIndex,
  flattenLibrary,
  flattenTree,
  isUnder,
  lastIndex,
  nameSegments,
  parseRowId,
  pruneExpanded,
  rowId,
  stemOf,
  stepIndex,
  type Listings,
  type TreeRow,
} from "./tree-model"

// A folder the user added, so its Readme.txt still shows as a row Windfall
// cannot use; the shipped factory library hides such files (tested below).
const FACTORY: BrowserRoot = { name: "Factory", path: "/f", kind: "user" }
const MINE: BrowserRoot = { name: "Mine", path: "/m", kind: "user" }

const entry = (
  path: string,
  kind: BrowserEntry["kind"] = "audio"
): BrowserEntry => ({ name: path.split("/").pop() ?? path, path, kind })

const ready = (entries: BrowserEntry[]) => ({
  status: "ready" as const,
  entries,
})

const LISTINGS: Listings = {
  "/f": ready([entry("/f/Drums", "folder"), entry("/f/Loops", "folder")]),
  "/f/Drums": ready([
    entry("/f/Drums/Kicks", "folder"),
    entry("/f/Drums/Snares", "folder"),
    entry("/f/Drums/Readme.txt", "other"),
  ]),
  "/f/Drums/Kicks": ready([
    entry("/f/Drums/Kicks/Kick 01.wav"),
    entry("/f/Drums/Kicks/Kick 02.wav"),
  ]),
  "/f/Drums/Snares": ready([entry("/f/Drums/Snares/Snare 01.wav")]),
  "/f/Loops": ready([entry("/f/Loops/Drum loop 128.wav")]),
}

const open = (...paths: string[]) =>
  new Set(paths.map((path) => rowId("/f", path)))

function names(rows: TreeRow[]): string[] {
  return rows.map((row) =>
    row.type === "entry"
      ? `${"  ".repeat(row.depth)}${row.name}`
      : `${"  ".repeat(row.depth)}(${row.status})`
  )
}

function flat(expanded: ReadonlySet<string>, filter = "", listings = LISTINGS) {
  return flattenTree({ roots: [FACTORY, MINE], listings, expanded, filter })
}

describe("flattenTree", () => {
  it("lists only the roots when nothing is open", () => {
    const tree = flat(new Set())
    expect(names(tree.rows)).toEqual(["Factory", "Mine"])
    expect(tree.rows[0]).toMatchObject({
      type: "entry",
      depth: 0,
      open: false,
      parent: null,
      root: FACTORY,
      position: 1,
      setSize: 2,
    })
  })

  it("lists open folders depth first, with depth and place among siblings", () => {
    const tree = flat(open("/f", "/f/Drums", "/f/Drums/Kicks"))
    expect(names(tree.rows)).toEqual([
      "Factory",
      "  Drums",
      "    Kicks",
      "      Kick 01.wav",
      "      Kick 02.wav",
      "    Snares",
      "    Readme.txt",
      "  Loops",
      "Mine",
    ])
    const kick = tree.rows[4]
    expect(kick).toMatchObject({
      parent: rowId("/f", "/f/Drums/Kicks"),
      parentPath: "/f/Drums/Kicks",
      position: 2,
      setSize: 2,
      root: null,
    })
    expect(tree.indexOf.get(rowId("/f", "/f/Loops"))).toBe(7)
  })

  it("keeps a folder closed when only a folder inside it is marked open", () => {
    const tree = flat(open("/f", "/f/Drums/Kicks"))
    expect(names(tree.rows)).toEqual(["Factory", "  Drums", "  Loops", "Mine"])
  })

  it("puts a line under an open folder that is loading, failed or empty", () => {
    const listings: Listings = {
      ...LISTINGS,
      "/f/Drums": { status: "loading", entries: null },
      "/f/Loops": { status: "error", message: "Access denied" },
      "/m": ready([]),
    }
    const expanded = new Set([...open("/f", "/f/Drums", "/f/Loops")])
    expanded.add(rowId("/m", "/m"))
    const tree = flat(expanded, "", listings)
    expect(names(tree.rows)).toEqual([
      "Factory",
      "  Drums",
      "    (loading)",
      "  Loops",
      "    (error)",
      "Mine",
      "  (empty)",
    ])
    expect(tree.rows[1]).toMatchObject({ busy: true, failed: false })
    expect(tree.rows[3]).toMatchObject({ busy: false, failed: true })
    expect(tree.rows[4]).toMatchObject({
      message: "Access denied",
      path: "/f/Loops",
    })
  })

  it("treats an open folder that was never read as loading", () => {
    const tree = flat(new Set([rowId("/m", "/m")]))
    expect(names(tree.rows)).toEqual(["Factory", "Mine", "  (loading)"])
  })

  it("keeps the old rows on screen while a folder is read again", () => {
    const listings: Listings = {
      ...LISTINGS,
      "/f": { status: "loading", entries: [entry("/f/Loops", "folder")] },
    }
    const tree = flat(open("/f"), "", listings)
    expect(names(tree.rows)).toEqual(["Factory", "  Loops", "Mine"])
    expect(tree.rows[0]).toMatchObject({ busy: true })
  })

  it("gives the same folder a different row under each root", () => {
    const inner: BrowserRoot = { name: "Kicks", path: "/f/Drums", kind: "user" }
    const tree = flattenTree({
      roots: [FACTORY, inner],
      listings: LISTINGS,
      expanded: open("/f"),
      filter: "",
    })
    const ids = tree.rows.map((row) => row.id)
    expect(new Set(ids).size).toBe(ids.length)
    expect(names(tree.rows)).toEqual(["Factory", "  Drums", "  Loops", "Kicks"])
  })
})

describe("flattenTree with a filter", () => {
  it("keeps matches and their ancestors, whatever is open", () => {
    const tree = flat(new Set(), "snare")
    expect(names(tree.rows)).toEqual([
      "Factory",
      "  Drums",
      "    Snares",
      "      Snare 01.wav",
    ])
    expect(tree.filtering).toBe(true)
    expect(tree.matchCount).toBe(2)
    expect(tree.rows[1]).toMatchObject({ open: true, forced: true })
  })

  it("ignores case and surrounding spaces, and says where the match is", () => {
    const tree = flat(new Set(), "  KICK 02 ")
    expect(names(tree.rows)).toEqual([
      "Factory",
      "  Drums",
      "    Kicks",
      "      Kick 02.wav",
    ])
    expect(tree.rows[3]).toMatchObject({ match: { start: 0, end: 7 } })
    expect(tree.rows[2]).toMatchObject({ match: null })
  })

  it("lets a folder whose own name matches keep all of its contents", () => {
    // "Drums" matches; nothing inside it does, so it opens only if the user
    // opened it, and then shows everything.
    const closed = flat(new Set(), "drums")
    expect(names(closed.rows)).toEqual(["Factory", "  Drums"])
    expect(closed.rows[1]).toMatchObject({ open: false, forced: false })

    const opened = flat(open("/f/Drums"), "drums")
    expect(names(opened.rows)).toEqual([
      "Factory",
      "  Drums",
      "    Kicks",
      "    Snares",
      "    Readme.txt",
    ])
  })

  it("finds matches in folders that were read but are closed", () => {
    const tree = flat(new Set(), "loop")
    expect(names(tree.rows)).toEqual([
      "Factory",
      "  Loops",
      "    Drum loop 128.wav",
    ])
  })

  it("returns no rows when nothing matches", () => {
    const tree = flat(open("/f"), "zzz")
    expect(tree.rows).toEqual([])
    expect(tree.matchCount).toBe(0)
  })

  it("does not look into folders that were never read", () => {
    const listings: Listings = { "/f": LISTINGS["/f"] }
    const tree = flat(open("/f"), "kick", listings)
    expect(tree.rows).toEqual([])
  })

  it("handles a 10,000-file folder", () => {
    const files = Array.from({ length: 10_000 }, (_, index) =>
      entry(`/m/Sound ${String(index).padStart(5, "0")}.wav`)
    )
    const listings: Listings = { "/m": ready(files) }
    const expanded = new Set([rowId("/m", "/m")])
    const all = flattenTree({ roots: [MINE], listings, expanded, filter: "" })
    expect(all.rows).toHaveLength(10_001)
    const some = flattenTree({
      roots: [MINE],
      listings,
      expanded,
      filter: "0999",
    })
    // Sound 00999 and Sound 09990 to 09999.
    expect(some.rows).toHaveLength(12)
  })
})

describe("factory support files", () => {
  const SHIPPED: BrowserRoot = { name: "Factory", path: "/s", kind: "factory" }
  const listings: Listings = {
    "/s": ready([
      entry("/s/Loops", "folder"),
      entry("/s/assets", "folder"),
      entry("/s/LICENSE.md", "other"),
      entry("/s/README.md", "other"),
    ]),
    "/s/Loops": ready([
      entry("/s/Loops/Afterglow.wav"),
      entry("/s/Loops/manifest.json", "other"),
    ]),
    "/s/assets": ready([entry("/s/assets/GeneralUser-GS-LICENSE.txt", "other")]),
  }

  it("leaves licences, readmes and manifests out of the factory library", () => {
    const tree = flattenTree({
      roots: [SHIPPED],
      listings,
      expanded: new Set(
        ["/s", "/s/Loops", "/s/assets"].map((path) => rowId("/s", path))
      ),
      filter: "",
    })
    expect(names(tree.rows)).toEqual([
      "Factory",
      "  Loops",
      "    Afterglow.wav",
      "  assets",
      "    (empty)",
    ])
    expect(tree.rows[1]).toMatchObject({ position: 1, setSize: 2 })
  })

  it("leaves files Windfall cannot use out of search results", () => {
    const token = (path: string) => ({ rootPath: "/s", path })
    const held = (path: string, kind: BrowserEntry["kind"]) => ({
      entry: entry(path, kind),
      token: token(path),
      relativePath: path.slice(3),
    })
    const tree = flattenLibrary({
      entries: [
        held("/s/Loops/Afterglow.wav", "audio"),
        held("/s/README.md", "other"),
      ],
    } as unknown as Parameters<typeof flattenLibrary>[0])
    expect(tree.rows.map((row) => row.type === "entry" && row.name)).toEqual([
      "Afterglow.wav",
    ])
    expect(tree.rows[0]).toMatchObject({ position: 1, setSize: 1 })
  })
})

describe("moving through rows", () => {
  const rows = flat(open("/f", "/f/Drums")).rows
  // Factory, Drums, Kicks, Snares, Readme.txt, Loops, Mine

  it("steps over files Windfall cannot use", () => {
    expect(stepIndex(rows, 3, 1)).toBe(5)
    expect(stepIndex(rows, 5, -1)).toBe(3)
  })

  it("stops at the ends", () => {
    expect(stepIndex(rows, 0, -1)).toBe(-1)
    expect(stepIndex(rows, 6, 1)).toBe(-1)
    expect(firstIndex(rows)).toBe(0)
    expect(lastIndex(rows)).toBe(6)
  })

  it("moves several rows at once and stops at the last one it can reach", () => {
    expect(stepIndex(rows, 0, 1, 3)).toBe(3)
    expect(stepIndex(rows, 0, 1, 50)).toBe(6)
    expect(stepIndex(rows, 6, -1, 50)).toBe(0)
  })

  it("skips status lines", () => {
    const withStatus = flat(new Set([rowId("/m", "/m")])).rows
    // Factory, Mine, (loading)
    expect(stepIndex(withStatus, 1, 1)).toBe(-1)
    expect(lastIndex(withStatus)).toBe(1)
  })
})

describe("findByPrefix", () => {
  const rows = flat(open("/f", "/f/Drums", "/f/Drums/Kicks")).rows
  // 0 Factory, 1 Drums, 2 Kicks, 3 Kick 01, 4 Kick 02, 5 Snares,
  // 6 Readme.txt, 7 Loops, 8 Mine

  it("jumps to the next row starting with a letter, and cycles", () => {
    expect(findByPrefix(rows, 0, "k")).toBe(2)
    expect(findByPrefix(rows, 2, "k")).toBe(3)
    expect(findByPrefix(rows, 4, "k")).toBe(2)
  })

  it("refines from the current row as more letters arrive", () => {
    expect(findByPrefix(rows, 2, "ki")).toBe(2)
    expect(findByPrefix(rows, 2, "kick 02")).toBe(4)
  })

  it("ignores case and starts from the top with no selection", () => {
    expect(findByPrefix(rows, -1, "F")).toBe(0)
    expect(findByPrefix(rows, -1, "LOO")).toBe(7)
  })

  it("never lands on a file Windfall cannot use", () => {
    expect(findByPrefix(rows, 0, "r")).toBe(-1)
  })

  it("returns -1 when nothing starts that way", () => {
    expect(findByPrefix(rows, 0, "zz")).toBe(-1)
    expect(findByPrefix([], -1, "a")).toBe(-1)
  })
})

describe("paths and ids", () => {
  it("tells whether a path is inside a folder, for both kinds of slash", () => {
    expect(isUnder("/a/b/c.wav", "/a/b")).toBe(true)
    expect(isUnder("/a/b", "/a/b")).toBe(false)
    expect(isUnder("/a/bc/d.wav", "/a/b")).toBe(false)
    expect(isUnder("C:\\Samples\\Kick.wav", "C:\\Samples")).toBe(true)
    expect(isUnder("C:\\Samples2\\Kick.wav", "C:\\Samples")).toBe(false)
    expect(isUnder("C:\\Kick.wav", "C:\\")).toBe(true)
  })

  it("round-trips a row id", () => {
    expect(parseRowId(rowId("/f", "/f/Drums"))).toEqual({
      root: "/f",
      path: "/f/Drums",
    })
    expect(parseRowId("no separator")).toBeNull()
  })
})

describe("pruneExpanded", () => {
  const expanded = open("/f", "/f/Drums", "/f/Drums/Kicks", "/f/Old/Deep")

  it("drops folders the fresh listing no longer has, with what was inside", () => {
    const kept = pruneExpanded(expanded, "/f", [
      entry("/f/Drums", "folder"),
      entry("/f/Loops", "folder"),
    ])
    expect([...kept].sort()).toEqual(
      [...open("/f", "/f/Drums", "/f/Drums/Kicks")].sort()
    )
  })

  it("does not keep a folder because a file took its name", () => {
    const kept = pruneExpanded(expanded, "/f/Drums", [
      entry("/f/Drums/Kicks", "audio"),
    ])
    expect(kept.has(rowId("/f", "/f/Drums/Kicks"))).toBe(false)
  })

  it("returns the same set when nothing was dropped", () => {
    const same = pruneExpanded(open("/f", "/f/Drums"), "/f", [
      entry("/f/Drums", "folder"),
    ])
    expect(same).toEqual(open("/f", "/f/Drums"))
    const input = open("/f")
    expect(pruneExpanded(input, "/elsewhere", [])).toBe(input)
  })

  it("drops ids it cannot read", () => {
    expect(pruneExpanded(new Set(["garbage"]), "/f", []).size).toBe(0)
  })
})

describe("names", () => {
  it("finds the extension of files Windfall opens", () => {
    expect(extensionStart("Kick 01.wav", "audio")).toBe(7)
    expect(extensionStart("Song.v2.windfall", "project")).toBe(7)
    expect(extensionStart("Samples.old", "folder")).toBe(11)
    expect(extensionStart(".hidden", "audio")).toBe(7)
    expect(stemOf("Kick 01.wav", "audio")).toBe("Kick 01")
    expect(stemOf("Drums", "folder")).toBe("Drums")
  })

  it("cuts a name at the match and at the extension", () => {
    expect(nameSegments("Kick 01.wav", "audio", { start: 0, end: 4 })).toEqual([
      { text: "Kick", marked: true, extension: false },
      { text: " 01", marked: false, extension: false },
      { text: ".wav", marked: false, extension: true },
    ])
  })

  it("marks a match that runs into the extension", () => {
    expect(nameSegments("Kick.wav", "audio", { start: 2, end: 6 })).toEqual([
      { text: "Ki", marked: false, extension: false },
      { text: "ck", marked: true, extension: false },
      { text: ".w", marked: true, extension: true },
      { text: "av", marked: false, extension: true },
    ])
  })

  it("returns the whole name as one run when there is nothing to mark", () => {
    expect(nameSegments("Drums", "folder", null)).toEqual([
      { text: "Drums", marked: false, extension: false },
    ])
  })
})
