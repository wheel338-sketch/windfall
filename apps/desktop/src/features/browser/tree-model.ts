import type {
  BrowserEntry,
  BrowserEntryKind,
  BrowserRoot,
  LibraryFileToken,
  LibraryResults,
} from "@/bindings"

/** What is known about the contents of one folder. */
export type Listing =
  /** `entries` holds the previous contents while a folder is read again. */
  | { status: "loading"; entries: BrowserEntry[] | null }
  | { status: "ready"; entries: BrowserEntry[] }
  | { status: "error"; message: string }

/** Folder contents by absolute path. */
export type Listings = Readonly<Record<string, Listing | undefined>>

export type MatchRange = { start: number; end: number }

export type EntryRow = {
  library?: LibraryFileToken
  relativePath?: string
  type: "entry"
  id: string
  path: string
  name: string
  kind: BrowserEntryKind
  depth: number
  /** Id of the row of the folder this one is in. */
  parent: string | null
  /** Path of the folder this one is in. */
  parentPath: string | null
  /** Set on the top-level rows. */
  root: BrowserRoot | null
  /** Folders only: showing its contents. */
  open: boolean
  /** Open because the filter found something inside, whatever the user chose. */
  forced: boolean
  /** Its contents are being read. */
  busy: boolean
  /** Its contents could not be read. */
  failed: boolean
  /** Where the filter text sits in the name. */
  match: MatchRange | null
  /** Place among the rows of the same folder, counting from 1. */
  position: number
  setSize: number
}

/**
 * Indexed results retain the same row identity, selection, keyboard and drag
 * semantics. A search looks for something to use, so files Windfall cannot
 * use, such as licences, readmes and manifests, are not among the results.
 */
export function flattenLibrary(results: LibraryResults | null): FlatTree {
  const entries = (results?.entries ?? []).filter(
    (held) => held.entry.kind !== "other"
  )
  const rows: EntryRow[] = entries.map((held, index) => ({
    ...held.entry,
    library: held.token,
    relativePath: held.relativePath,
    type: "entry",
    id: rowId(held.token.rootPath, held.entry.path),
    depth: 0,
    parent: null,
    parentPath: held.entry.path.slice(
      0,
      Math.max(
        held.entry.path.lastIndexOf("/"),
        held.entry.path.lastIndexOf("\\")
      )
    ),
    root: null,
    open: false,
    forced: false,
    busy: false,
    failed: false,
    match: null,
    position: index + 1,
    setSize: entries.length,
  }))
  return {
    rows,
    indexOf: new Map(rows.map((r, i) => [r.id, i])),
    filtering: true,
    matchCount: rows.length,
  }
}

/** A line under an open folder that has nothing to list. */
export type StatusRow = {
  type: "status"
  id: string
  parent: string
  /** Path of the folder the line is about. */
  path: string
  depth: number
  status: "loading" | "error" | "empty"
  message: string | null
}

export type TreeRow = EntryRow | StatusRow

export type TreeInput = {
  roots: readonly BrowserRoot[]
  listings: Listings
  /** Row ids of the folders the user has open. */
  expanded: ReadonlySet<string>
  filter: string
}

export type FlatTree = {
  rows: TreeRow[]
  indexOf: Map<string, number>
  filtering: boolean
  matchCount: number
}

// A path cannot contain this character on any file system.
const ID_SEPARATOR = "\u0000"

/**
 * A row's id is its path together with the root it is shown under. The same
 * folder can show twice when one root sits inside another, and each showing
 * opens and closes on its own.
 */
export function rowId(rootPath: string, path: string): string {
  return `${rootPath}${ID_SEPARATOR}${path}`
}

export function parseRowId(id: string): { root: string; path: string } | null {
  const cut = id.indexOf(ID_SEPARATOR)
  if (cut < 0) return null
  return { root: id.slice(0, cut), path: id.slice(cut + 1) }
}

function isSeparator(character: string | undefined): boolean {
  return character === "/" || character === "\\"
}

/** True when `path` is somewhere inside the folder `ancestor`. */
export function isUnder(path: string, ancestor: string): boolean {
  if (path.length <= ancestor.length || !path.startsWith(ancestor)) return false
  return (
    isSeparator(ancestor[ancestor.length - 1]) ||
    isSeparator(path[ancestor.length])
  )
}

/** The entries to show for a folder, or null when there are none to show yet. */
export function loadedEntries(
  listing: Listing | undefined
): BrowserEntry[] | null {
  if (!listing || listing.status === "error") return null
  return listing.entries
}

/** Where `query` (already lowercase) sits in `name`, ignoring case. */
export function findMatch(name: string, query: string): MatchRange | null {
  if (query === "") return null
  const start = name.toLowerCase().indexOf(query)
  return start < 0 ? null : { start, end: start + query.length }
}

export function normalizeFilter(filter: string): string {
  return filter.trim().toLowerCase()
}

/** Tells whether anything inside a folder, at any depth read so far, matches. */
function createMatchBelow(listings: Listings, query: string) {
  const memo = new Map<string, boolean>()
  const below = (path: string): boolean => {
    const known = memo.get(path)
    if (known !== undefined) return known
    // Set first, so a folder that lists itself cannot loop forever.
    memo.set(path, false)
    let found = false
    for (const entry of loadedEntries(listings[path]) ?? []) {
      if (
        entry.name.toLowerCase().includes(query) ||
        (entry.kind === "folder" && below(entry.path))
      ) {
        found = true
        break
      }
    }
    memo.set(path, found)
    return found
  }
  return below
}

/**
 * Whether a folder's entry gets a row. The factory library ships support
 * files beside its sounds, such as its licence, readme and loop manifest;
 * they are never something to play, so they are left out. A folder the user
 * added shows everything in it, with files Windfall cannot use greyed out.
 */
function listable(entry: BrowserEntry, root: BrowserRoot): boolean {
  return !(root.kind === "factory" && entry.kind === "other")
}

/**
 * Turns the folders read so far into the list of rows to draw, top to bottom.
 *
 * With a filter, a row stays when its name matches or when something inside
 * it does. A folder that holds a match is shown open with only the matching
 * branches. A folder whose own name matches keeps all of its contents, so
 * finding "Kicks" still lets you browse what is in it.
 */
export function flattenTree(input: TreeInput): FlatTree {
  const { roots, listings, expanded } = input
  const query = normalizeFilter(input.filter)
  const filtering = query !== ""
  const below = filtering ? createMatchBelow(listings, query) : null
  const rows: TreeRow[] = []
  let matchCount = 0

  const visit = (
    entries: readonly BrowserEntry[],
    rootOf: (entry: BrowserEntry, index: number) => BrowserRoot,
    parent: EntryRow | null,
    depth: number,
    showAll: boolean
  ) => {
    const shown: { entry: BrowserEntry; root: BrowserRoot }[] = []
    entries.forEach((entry, index) => {
      const root = rootOf(entry, index)
      if (!listable(entry, root)) return
      const visible =
        below === null ||
        showAll ||
        entry.name.toLowerCase().includes(query) ||
        (entry.kind === "folder" && below(entry.path))
      if (visible) shown.push({ entry, root })
    })

    shown.forEach(({ entry, root }, index) => {
      const isFolder = entry.kind === "folder"
      const match = findMatch(entry.name, query)
      if (match) matchCount += 1
      const id = rowId(root.path, entry.path)
      const forced = isFolder && below !== null && below(entry.path)
      const open = isFolder && (forced || expanded.has(id))
      const listing = listings[entry.path]
      const row: EntryRow = {
        type: "entry",
        id,
        path: entry.path,
        name: entry.name,
        kind: entry.kind,
        depth,
        parent: parent?.id ?? null,
        parentPath: parent?.path ?? null,
        root: parent === null ? root : null,
        open,
        forced,
        busy: open && (listing === undefined || listing.status === "loading"),
        failed: open && listing?.status === "error",
        match,
        position: index + 1,
        setSize: shown.length,
      }
      rows.push(row)
      if (!open) return

      const children =
        loadedEntries(listing)?.filter((child) => listable(child, root)) ??
        null
      if (children !== null && children.length > 0) {
        visit(children, () => root, row, depth + 1, showAll || match !== null)
        return
      }
      rows.push({
        type: "status",
        id: `${id}${ID_SEPARATOR}status`,
        parent: id,
        path: entry.path,
        depth: depth + 1,
        status:
          listing?.status === "error"
            ? "error"
            : children === null
              ? "loading"
              : "empty",
        message: listing?.status === "error" ? listing.message : null,
      })
    })
  }

  visit(
    roots.map((root) => ({ name: root.name, path: root.path, kind: "folder" })),
    (_entry, index) => roots[index],
    null,
    0,
    false
  )

  const indexOf = new Map<string, number>()
  rows.forEach((row, index) => indexOf.set(row.id, index))
  return { rows, indexOf, filtering, matchCount }
}

/** Rows the selection can land on. Files Windfall cannot use are skipped. */
export function isNavigable(row: TreeRow | null | undefined): row is EntryRow {
  return row?.type === "entry" && row.kind !== "other"
}

/**
 * The nearest row the selection can move to from `from`, going down (1) or
 * up (-1) by up to `distance` selectable rows. Returns -1 when there is none.
 */
export function stepIndex(
  rows: readonly TreeRow[],
  from: number,
  direction: 1 | -1,
  distance = 1
): number {
  let found = -1
  let left = distance
  for (
    let index = from + direction;
    index >= 0 && index < rows.length && left > 0;
    index += direction
  ) {
    if (isNavigable(rows[index])) {
      found = index
      left -= 1
    }
  }
  return found
}

export function firstIndex(rows: readonly TreeRow[]): number {
  return stepIndex(rows, -1, 1)
}

export function lastIndex(rows: readonly TreeRow[]): number {
  return stepIndex(rows, rows.length, -1)
}

/**
 * The row type-ahead should jump to: the next one whose name starts with what
 * was typed. A single letter moves on from the current row, so pressing it
 * again cycles; more letters keep refining from the current row.
 */
export function findByPrefix(
  rows: readonly TreeRow[],
  from: number,
  typed: string
): number {
  const prefix = typed.toLowerCase()
  if (prefix === "" || rows.length === 0) return -1
  const start = Math.max(0, from) + (typed.length === 1 && from >= 0 ? 1 : 0)
  for (let offset = 0; offset < rows.length; offset += 1) {
    const index = (start + offset) % rows.length
    const row = rows[index]
    if (isNavigable(row) && row.name.toLowerCase().startsWith(prefix)) {
      return index
    }
  }
  return -1
}

/**
 * Drops open-folder ids that point into `folderPath` at something its fresh
 * listing no longer has. Returns the same set when nothing was dropped.
 */
export function pruneExpanded(
  expanded: ReadonlySet<string>,
  folderPath: string,
  entries: readonly BrowserEntry[]
): ReadonlySet<string> {
  const folders = entries.filter((entry) => entry.kind === "folder")
  const kept = new Set<string>()
  for (const id of expanded) {
    const path = parseRowId(id)?.path
    if (path === undefined) continue
    const alive =
      !isUnder(path, folderPath) ||
      folders.some(
        (folder) => path === folder.path || isUnder(path, folder.path)
      )
    if (alive) kept.add(id)
  }
  return kept.size === expanded.size ? expanded : kept
}

/** Index of the dot before a file's extension, or the name's length. */
export function extensionStart(name: string, kind: BrowserEntryKind): number {
  if (kind !== "audio" && kind !== "project") return name.length
  const dot = name.lastIndexOf(".")
  return dot > 0 ? dot : name.length
}

/** A file's name without its extension. */
export function stemOf(name: string, kind: BrowserEntryKind): string {
  return name.slice(0, extensionStart(name, kind))
}

export type NameSegment = { text: string; marked: boolean; extension: boolean }

/** Cuts a name into runs that are drawn alike: matched or not, extension or not. */
export function nameSegments(
  name: string,
  kind: BrowserEntryKind,
  match: MatchRange | null
): NameSegment[] {
  const extension = extensionStart(name, kind)
  const cuts = new Set([0, extension, name.length])
  if (match) {
    cuts.add(match.start)
    cuts.add(match.end)
  }
  const points = [...cuts].sort((a, b) => a - b)
  const segments: NameSegment[] = []
  for (let index = 0; index < points.length - 1; index += 1) {
    const from = points[index]
    const to = points[index + 1]
    segments.push({
      text: name.slice(from, to),
      marked: match !== null && from >= match.start && to <= match.end,
      extension: from >= extension,
    })
  }
  return segments
}
