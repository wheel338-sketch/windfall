import type {
  BrowserRoot,
  LibraryEntry,
  LibraryFileToken,
  LibraryMetadata,
  LibraryResults,
  LibrarySearch,
} from "@/bindings"

import { listFolder } from "./browser"
import { sim } from "./wasm"
import { translatedText } from "@/lib/i18n"

const METADATA_KEY = "windfall.mock.browser-library"
const ROOTS_KEY = "windfall.mock.browser-roots"
const STALE =
  "This library result is out of date. Refresh the library and select the file again."
const LIMITATION =
  "Browser mode searches the fixture library. Reading your filesystem, permissions, symbolic links and drive changes requires the desktop app."
type StoragePort = Pick<Storage, "getItem" | "setItem"> | null

export function storedBrowserRoots(storage: StoragePort): BrowserRoot[] {
  try {
    const value: unknown = JSON.parse(storage?.getItem(ROOTS_KEY) ?? "[]")
    if (!Array.isArray(value)) return []
    return value
      .slice(0, 127)
      .filter(
        (r): r is BrowserRoot =>
          typeof r === "object" &&
          r !== null &&
          typeof r.path === "string" &&
          r.path.length <= 4096 &&
          r.path.startsWith("/") &&
          typeof r.name === "string" &&
          r.kind === "user"
      )
  } catch {
    return []
  }
}

/** The same query/tag Rust operations as native, over deliberately fictional fixtures. */
export function createLibraryMock(
  storage: StoragePort,
  roots: () => BrowserRoot[]
) {
  let generation = 1
  let metadata: Record<string, LibraryMetadata> = Object.create(null)
  let damaged: string | null = null
  try {
    const raw = storage?.getItem(METADATA_KEY)
    if (raw) {
      if (raw.length > 8 * 1024 * 1024)
        throw new Error(translatedText("metadata exceeds 8 MiB"))
      const value: unknown = JSON.parse(raw)
      if (
        !value ||
        typeof value !== "object" ||
        Array.isArray(value) ||
        Object.keys(value).length > 10_000
      )
        throw new Error(translatedText("invalid metadata"))
      for (const [path, held] of Object.entries(value)) {
        if (
          path.length > 4096 ||
          !held ||
          typeof held !== "object" ||
          typeof held.favorite !== "boolean" ||
          !Array.isArray(held.tags) ||
          !held.tags.every((t: unknown) => typeof t === "string")
        )
          throw new Error(translatedText("invalid metadata entry"))
        metadata[path] = {
          favorite: held.favorite,
          tags: sim.call<string[]>("browser_tags", 0, held.tags),
        }
      }
    }
  } catch {
    damaged =
      "Could not read browser library metadata. Repair or remove windfall.mock.browser-library in this browser's storage before saving favorites or tags. The original value has been preserved."
    metadata = Object.create(null)
  }

  function file(path: string): LibraryFileToken {
    const root = roots().find((r) => path.startsWith(`${r.path}/`))
    if (!root) throw new Error(translatedText(STALE))
    const entry = listFolder(
      roots(),
      path.slice(0, path.lastIndexOf("/"))
    ).find((e) => e.path === path && e.kind !== "folder")
    if (!entry) throw new Error(translatedText(STALE))
    return { path, rootPath: root.path, generation, fingerprint: "fixture-v1" }
  }

  function check(path: string, token?: LibraryFileToken) {
    if (!token) return
    const current = file(path)
    if (
      current.path !== token.path ||
      current.rootPath !== token.rootPath ||
      current.generation !== token.generation ||
      (token.fingerprint !== "" && current.fingerprint !== token.fingerprint)
    )
      throw new Error(translatedText(STALE))
  }

  return {
    file,
    check,
    changedRoots(next: BrowserRoot[]) {
      try {
        storage?.setItem(
          ROOTS_KEY,
          JSON.stringify(next.filter((r) => r.kind === "user"))
        )
      } catch {
        throw new Error(
          translatedText("Could not save browser folders. Check browser storage space and access, then try again.")
        )
      }
      generation += 1
    },
    refresh() {
      generation += 1
    },
    cancel(requestedGeneration: number) {
      if (requestedGeneration !== generation) return
      /* The fixture tree is scanned synchronously before the IPC reply. */
    },
    metadata(path: string) {
      file(path)
      if (damaged) throw new Error(damaged)
      return metadata[path] ?? { favorite: false, tags: [] }
    },
    setMetadata(path: string, next: LibraryMetadata) {
      file(path)
      if (damaged) throw new Error(damaged)
      const tags = sim.call<string[]>("browser_tags", 0, next.tags)
      const value = { favorite: next.favorite, tags }
      const updated = { ...metadata }
      if (!value.favorite && tags.length === 0) delete updated[path]
      else updated[path] = value
      if (Object.keys(updated).length > 10_000)
        throw new Error(
          translatedText("The library has reached its 10,000 annotated-file limit.")
        )
      const encoded = JSON.stringify(updated)
      if (encoded.length > 8 * 1024 * 1024)
        throw new Error(translatedText("The library metadata has reached its 8 MiB limit."))
      try {
        storage?.setItem(METADATA_KEY, encoded)
      } catch {
        throw new Error(
          translatedText("Could not save favorites and tags. Check browser storage space and access, then try again.")
        )
      }
      metadata = updated
      return value
    },
    search(search: LibrarySearch): LibraryResults {
      const tags = sim.call<string[]>("browser_tags", 0, search.tags)
      const all: LibraryEntry[] = []
      const seen = new Set<string>()
      for (const root of roots()) {
        const visit = (path: string) => {
          for (const entry of listFolder(roots(), path)) {
            if (seen.has(entry.path)) continue
            seen.add(entry.path)
            if (entry.kind === "folder") visit(entry.path)
            else
              all.push({
                entry,
                relativePath: entry.path.slice(root.path.length + 1),
                token: file(entry.path),
                metadata: metadata[entry.path] ?? { favorite: false, tags: [] },
              })
          }
        }
        visit(root.path)
      }
      const indices = sim.call<number[]>("browser_query", 0, {
        query: search.query,
        paths: all.map((e) => e.entry.path),
      })
      const matches = indices
        .map((i) => all[i])
        .filter(
          (e) =>
            (!search.favoritesOnly || e.metadata.favorite) &&
            tags.every((t) => e.metadata.tags.includes(t))
        )
      matches.sort(
        (a, b) =>
          a.relativePath.localeCompare(b.relativePath, "en", {
            numeric: true,
            sensitivity: "base",
          }) || a.entry.path.localeCompare(b.entry.path)
      )
      return {
        generation,
        status: "ready",
        indexed: all.length,
        examined: seen.size,
        entries: matches.slice(0, 500),
        truncated: false,
        resultsTruncated: matches.length > 500,
        availableTags: [...new Set(all.flatMap((e) => e.metadata.tags))]
          .sort()
          .slice(0, 256),
        issues: damaged ? [damaged] : [],
        limitation: LIMITATION,
      }
    },
  }
}
