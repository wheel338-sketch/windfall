import type { HistoryView, Pattern, Project, ProjectPatch } from "@/bindings"

/** The UI's copy of the document. */
export type DocumentState = {
  revision: number
  project: Project
  history: HistoryView
  dirty: boolean
  /** Path of the `.windfall` file, or null for a project never saved. */
  path: string | null
}

export type PatchOutcome =
  | { status: "applied"; state: DocumentState }
  /** The patch was applied before. */
  | { status: "ignored"; state: DocumentState }
  /** A patch in between was missed, so the snapshot must be fetched again. */
  | { status: "gap"; state: DocumentState }

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
}

function identity(item: unknown): unknown {
  if (!isRecord(item)) return undefined
  return item.id ?? item.channel
}

/**
 * Returns `next`, but with every part that equals the matching part of
 * `prev` replaced by the object from `prev`. A patch carries a whole section
 * even when one fader moved; this keeps the untouched channels, tracks and
 * notes at the same reference so their components do not render again.
 */
export function shareStructure<T>(prev: unknown, next: T): T {
  if (Object.is(prev, next)) return next

  if (Array.isArray(prev) && Array.isArray(next)) {
    const byIdentity = new Map<unknown, unknown>()
    for (const item of prev) {
      const key = identity(item)
      if (key !== undefined) byIdentity.set(key, item)
    }
    let same = prev.length === next.length
    const shared = next.map((item: unknown, index) => {
      const key = identity(item)
      const match = key === undefined ? prev[index] : byIdentity.get(key)
      const result = shareStructure(match, item)
      if (result !== prev[index]) same = false
      return result
    })
    return (same ? prev : shared) as T
  }

  if (isRecord(prev) && isRecord(next)) {
    const keys = Object.keys(next)
    let same = keys.length === Object.keys(prev).length
    const shared: Record<string, unknown> = {}
    for (const key of keys) {
      shared[key] = shareStructure(prev[key], next[key])
      if (shared[key] !== prev[key] || !(key in prev)) same = false
    }
    return (same ? prev : shared) as T
  }

  return next
}

function mergePatterns(current: Pattern[], patch: ProjectPatch): Pattern[] {
  if (!patch.patternOrder && !patch.patterns?.length) return current
  const byId = new Map(current.map((pattern) => [pattern.id, pattern]))
  for (const pattern of patch.patterns ?? []) {
    byId.set(pattern.id, shareStructure(byId.get(pattern.id), pattern))
  }
  const order = patch.patternOrder ?? current.map((pattern) => pattern.id)
  const merged = order.flatMap((id) => {
    const pattern = byId.get(id)
    return pattern ? [pattern] : []
  })
  return shareStructure(current, merged)
}

/**
 * Merges a patch into the UI's copy of the document. A patch applies only
 * when its revision is exactly one more than the revision held.
 */
export function applyPatch(
  state: DocumentState,
  patch: ProjectPatch
): PatchOutcome {
  if (patch.revision <= state.revision) return { status: "ignored", state }
  if (patch.revision !== state.revision + 1) return { status: "gap", state }

  const { project } = state
  const next: Project = {
    ...project,
    noteEditGroups: shareStructure(
      project.noteEditGroups,
      patch.noteEditGroups ?? project.noteEditGroups
    ),
    notebook: shareStructure(
      project.notebook,
      patch.notebook ?? project.notebook
    ),
    plugins: shareStructure(project.plugins, patch.plugins ?? project.plugins),
    settings: shareStructure(
      project.settings,
      patch.settings ?? project.settings
    ),
    samples: shareStructure(project.samples, patch.samples ?? project.samples),
    channels: shareStructure(
      project.channels,
      patch.channels ?? project.channels
    ),
    mixer: shareStructure(project.mixer, patch.mixer ?? project.mixer),
    playlist: shareStructure(
      project.playlist,
      patch.playlist ?? project.playlist
    ),
    automations: shareStructure(
      project.automations,
      patch.automations ?? project.automations
    ),
    patterns: mergePatterns(project.patterns, patch),
  }

  return {
    status: "applied",
    state: {
      revision: patch.revision,
      project: shareStructure(project, next),
      history: shareStructure(state.history, patch.history),
      dirty: patch.dirty,
      path: state.path,
    },
  }
}
