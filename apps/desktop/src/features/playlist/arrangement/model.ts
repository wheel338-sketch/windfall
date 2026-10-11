import type {
  Arrangement,
  ArrangementBook,
  ClipGroup,
  TrackGroup,
  TrackKind,
} from "@/bindings"
export type {
  Arrangement,
  ArrangementBook,
  ClipGroup,
  TrackGroup,
  TrackKind,
} from "@/bindings"

export interface NamedReference {
  id: number
  name: string
}
export function emptyArrangementBook(): ArrangementBook {
  return {
    arrangements: [],
    active: null,
    trackGroups: [],
    groupParents: {},
    trackParents: {},
    clipGroups: [],
    linkedTracks: {},
  }
}
export type ArrangementEdit =
  | { type: "addArrangement"; arrangement: Arrangement }
  | { type: "switchArrangement"; id: number }
  | { type: "renameArrangement"; id: number; name: string }
  | { type: "setReferences"; id: number; clips: number[]; tracks: number[] }
  | { type: "removeArrangement"; id: number }
  | { type: "addTrackGroup"; group: TrackGroup }
  | { type: "renameTrackGroup"; id: number; name: string }
  | { type: "moveTrackGroup"; id: number; parent: number | null }
  | { type: "moveTrack"; id: number; parent: number | null }
  | { type: "removeTrackGroup"; id: number }
  | { type: "addClipGroup"; group: ClipGroup }
  | { type: "removeClipGroup"; id: number }
  | { type: "linkTrack"; id: number; kind: TrackKind | null }

export class ArrangementEditError extends Error {
  readonly code:
    | "missingId"
    | "duplicateId"
    | "invalidName"
    | "lastArrangement"
    | "invalidActive"
    | "groupCycle"
    | "tooFewClips"
    | "clipAlreadyGrouped"
  constructor(
    code:
      | "missingId"
      | "duplicateId"
      | "invalidName"
      | "lastArrangement"
      | "invalidActive"
      | "groupCycle"
      | "tooFewClips"
      | "clipAlreadyGrouped",
    message: string
  ) {
    super(message)
    this.code = code
    this.name = "ArrangementEditError"
  }
}
function missing(kind: string, id: number): never {
  throw new ArrangementEditError(
    "missingId",
    `${kind} ID ${id} does not exist.`
  )
}
function nameCheck(name: string) {
  if (
    !name.trim() ||
    name.includes("\0") ||
    new TextEncoder().encode(name).length > 256
  )
    throw new ArrangementEditError(
      "invalidName",
      "Enter a name of up to 256 UTF-8 bytes with no NUL characters."
    )
}
function distinct(ids: number[]) {
  if (
    ids.some((id) => !Number.isInteger(id) || id < 0 || id > 0xffffffff) ||
    new Set(ids).size !== ids.length
  )
    throw new ArrangementEditError(
      "duplicateId",
      "IDs must be distinct unsigned 32-bit integers."
    )
}
export function checkArrangementBook(book: ArrangementBook) {
  distinct(book.arrangements.map((item) => item.id))
  if (
    (book.arrangements.length === 0) !== (book.active === null) ||
    (book.active !== null &&
      !book.arrangements.some((item) => item.id === book.active))
  )
    throw new ArrangementEditError(
      "invalidActive",
      "Choose an arrangement that exists."
    )
  for (const item of book.arrangements) {
    nameCheck(item.name)
    distinct(item.clips)
    distinct(item.tracks)
  }
  distinct(book.trackGroups.map((group) => group.id))
  const groups = new Set(book.trackGroups.map((group) => group.id))
  for (const group of book.trackGroups) nameCheck(group.name)
  for (const [child, parent] of Object.entries(book.groupParents)) {
    if (!groups.has(Number(child))) missing("Track group", Number(child))
    const seen = new Set([Number(child)])
    let cursor: number | undefined = parent
    while (cursor !== undefined) {
      if (!groups.has(cursor)) missing("Track group", cursor)
      if (seen.has(cursor))
        throw new ArrangementEditError(
          "groupCycle",
          "A group cannot be nested under itself or its descendants."
        )
      seen.add(cursor)
      cursor = book.groupParents[cursor]
    }
  }
  for (const parent of Object.values(book.trackParents))
    if (!groups.has(parent)) missing("Track group", parent)
  distinct(book.clipGroups.map((group) => group.id))
  const grouped = new Set<number>()
  for (const group of book.clipGroups) {
    if (group.clips.length < 2)
      throw new ArrangementEditError(
        "tooFewClips",
        "Choose at least two different clips."
      )
    distinct(group.clips)
    for (const clip of group.clips) {
      if (grouped.has(clip))
        throw new ArrangementEditError(
          "clipAlreadyGrouped",
          `Clip ${clip} already belongs to a group.`
        )
      grouped.add(clip)
    }
  }
}

/** Pure local editor; project authority remains in the Rust command seam. */
export function editArrangementBook(
  book: ArrangementBook,
  edit: ArrangementEdit,
  validLink: (kind: TrackKind) => boolean = () => true
): ArrangementBook {
  checkArrangementBook(book)
  const next = structuredClone(book)
  const arrangement = (id: number) =>
    next.arrangements.find((item) => item.id === id) ??
    missing("Arrangement", id)
  const group = (id: number) =>
    next.trackGroups.find((item) => item.id === id) ??
    missing("Track group", id)
  switch (edit.type) {
    case "addArrangement":
      next.arrangements.push(structuredClone(edit.arrangement))
      next.active ??= edit.arrangement.id
      break
    case "switchArrangement":
      arrangement(edit.id)
      next.active = edit.id
      break
    case "renameArrangement":
      arrangement(edit.id).name = edit.name
      break
    case "setReferences":
      Object.assign(arrangement(edit.id), {
        clips: [...edit.clips],
        tracks: [...edit.tracks],
      })
      break
    case "removeArrangement": {
      arrangement(edit.id)
      if (next.arrangements.length === 1)
        throw new ArrangementEditError(
          "lastArrangement",
          "The last arrangement cannot be deleted."
        )
      const index = next.arrangements.findIndex((item) => item.id === edit.id)
      next.arrangements.splice(index, 1)
      if (next.active === edit.id)
        next.active =
          next.arrangements[Math.min(index, next.arrangements.length - 1)].id
      break
    }
    case "addTrackGroup":
      next.trackGroups.push({ ...edit.group })
      break
    case "renameTrackGroup":
      group(edit.id).name = edit.name
      break
    case "moveTrackGroup":
      group(edit.id)
      if (edit.parent === null) delete next.groupParents[edit.id]
      else {
        group(edit.parent)
        next.groupParents[edit.id] = edit.parent
      }
      break
    case "moveTrack":
      if (edit.parent === null) delete next.trackParents[edit.id]
      else {
        group(edit.parent)
        next.trackParents[edit.id] = edit.parent
      }
      break
    case "removeTrackGroup": {
      group(edit.id)
      const parent = next.groupParents[edit.id]
      delete next.groupParents[edit.id]
      next.trackGroups = next.trackGroups.filter((item) => item.id !== edit.id)
      for (const map of [next.groupParents, next.trackParents]) {
        for (const [child, owner] of Object.entries(map)) {
          if (owner === edit.id) {
            if (parent === undefined) delete map[Number(child)]
            else map[Number(child)] = parent
          }
        }
      }
      break
    }
    case "addClipGroup":
      next.clipGroups.push(structuredClone(edit.group))
      break
    case "removeClipGroup":
      if (!next.clipGroups.some((item) => item.id === edit.id))
        missing("Clip group", edit.id)
      next.clipGroups = next.clipGroups.filter((item) => item.id !== edit.id)
      break
    case "linkTrack":
      if (edit.kind === null) delete next.linkedTracks[edit.id]
      else {
        if (!validLink(edit.kind))
          missing(
            edit.kind.type === "instrument"
              ? "Channel"
              : edit.kind.type === "audio"
                ? "Audio source"
                : "Mixer track",
            edit.kind.type === "instrument"
              ? edit.kind.channel
              : edit.kind.type === "audio"
                ? edit.kind.source
                : edit.kind.mixer
          )
        next.linkedTracks[edit.id] = { ...edit.kind }
      }
      break
  }
  checkArrangementBook(next)
  return next
}

export function parseReferences(value: string, available: number[]): number[] {
  if (!value.trim()) return []
  const ids = value.split(",").map((part) => {
    if (!/^\d+$/.test(part.trim())) missing("Reference", Number.NaN)
    return Number(part.trim())
  })
  distinct(ids)
  for (const id of ids) if (!available.includes(id)) missing("Reference", id)
  return ids
}
