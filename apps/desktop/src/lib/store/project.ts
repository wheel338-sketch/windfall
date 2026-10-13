import { create } from "zustand"

import type {
  Command,
  DispatchResult,
  DocumentSnapshot,
  Project,
  ProjectPatch,
} from "@/bindings"
import { reportError } from "@/lib/errors"
import { backend } from "@/lib/ipc"
import { FORMAT_VERSION, MASTER_TRACK } from "@/lib/units"

import { applyPatch, type DocumentState } from "./patch"
import { getProjectGeneration, onProjectReplaced } from "./replaced"
import { translatedText as t } from "@/lib/i18n"

/** Shown for the instant before the first snapshot arrives. */
const BLANK_PROJECT: Project = {
  formatVersion: FORMAT_VERSION,
  nextId: 1,
  settings: {
    name: "",
    tempoBpm: 140,
    timeSignature: { numerator: 4, denominator: 4 },
    swing: 0,
  },
  samples: [],
  channels: [],
  patterns: [],
  mixer: {
    tracks: [
      {
        id: MASTER_TRACK,
        name: "Master",
        color: 0x9ca3af,
        volume: 1,
        pan: 0,
        muted: false,
        solo: false,
        output: null,
        sends: [],
        effects: [],
      },
    ],
  },
  playlist: { tracks: [], clips: [] },
  automations: [],
  retainedPlugins: [],
}

export type ProjectState = DocumentState & {
  /** False until the first snapshot has been loaded. */
  ready: boolean
}

/**
 * The UI's mirror of the document. Nothing writes to it except snapshots and
 * patches from the backend; to change the project, call `dispatch`.
 */
export const useProjectStore = create<ProjectState>(() => ({
  ready: false,
  revision: 0,
  project: BLANK_PROJECT,
  history: { entries: [], cursor: 0 },
  dirty: false,
  path: null,
}))

export function loadSnapshot(snapshot: DocumentSnapshot) {
  useProjectStore.setState({
    ready: true,
    revision: snapshot.revision,
    project: snapshot.project,
    history: snapshot.history,
    dirty: snapshot.dirty,
    path: snapshot.path,
  })
}

let refetching: Promise<void> | null = null
/** The newest revision a patch has carried while a snapshot was on its way. */
let newestMissed = 0

/**
 * Replaces the whole copy at startup or after a missed patch. A newer
 * patch can overtake the in-flight snapshot; never roll it back, and fetch
 * again when that snapshot does not include the newest observed revision.
 */
export function refetchSnapshot(): Promise<void> {
  if (refetching) return refetching
  const generation = getProjectGeneration()
  refetching = backend
    .documentSnapshot()
    .then((snapshot) => {
      if (
        generation === getProjectGeneration() &&
        snapshot.revision >= useProjectStore.getState().revision
      )
        loadSnapshot(snapshot)
      return snapshot.revision
    })
    .catch((error: unknown) => {
      if (generation === getProjectGeneration())
        reportError(error, t("Could not load the project"))
      return Infinity
    })
    .then((revision) => {
      refetching = null
      const behind =
        generation !== getProjectGeneration() || newestMissed > revision
      newestMissed = 0
      if (behind) return refetchSnapshot()
    })
  return refetching
}

/**
 * Merges a patch from the backend. A patch that was already applied is
 * ignored; a gap in the revisions means one was missed, so the snapshot is
 * fetched again.
 */
export function receivePatch(patch: ProjectPatch) {
  if (refetching) newestMissed = Math.max(newestMissed, patch.revision)
  const outcome = applyPatch(useProjectStore.getState(), patch)
  if (outcome.status === "applied") {
    useProjectStore.setState(outcome.state)
  } else if (outcome.status === "gap") {
    newestMissed = Math.max(newestMissed, patch.revision)
    void refetchSnapshot()
  }
}

/** Waits for this edit, while recovery of later edits continues independently. */
async function waitForRevision(
  generation: number,
  revision: number
): Promise<boolean> {
  let unsubscribeProject = () => {}
  let unsubscribeGeneration = () => {}
  try {
    const mirrored = new Promise<boolean>((resolve) => {
      const check = () => {
        if (generation !== getProjectGeneration()) resolve(false)
        else if (useProjectStore.getState().revision >= revision) resolve(true)
      }
      unsubscribeProject = useProjectStore.subscribe(check)
      unsubscribeGeneration = onProjectReplaced(check)
      check()
    })
    return await Promise.race([
      mirrored,
      refetchSnapshot().then(
        () =>
          generation === getProjectGeneration() &&
          useProjectStore.getState().revision >= revision
      ),
    ])
  } finally {
    unsubscribeProject()
    unsubscribeGeneration()
  }
}

/**
 * Sends an edit to the backend. Pass the same `gesture` id for every edit of
 * one drag so they become a single undo step. Resolves to `null` when the
 * command fails, its document was replaced, or snapshot recovery fails.
 * A successful reply waits until the mirror includes its patch, so callers
 * can safely follow created ids even after a missed event.
 */
export async function dispatch(
  command: Command,
  gesture?: number
): Promise<DispatchResult | null> {
  const generation = getProjectGeneration()
  try {
    const result = await backend.dispatch(command, gesture)
    if (generation !== getProjectGeneration()) return null
    receivePatch(result.patch)
    if (useProjectStore.getState().revision < result.patch.revision) {
      if (!(await waitForRevision(generation, result.patch.revision)))
        return null
    }
    // A replacement loads its snapshot before announcing the new generation.
    // Recheck after any store notification or awaited recovery.
    if (generation !== getProjectGeneration()) return null
    return result
  } catch (error) {
    if (generation === getProjectGeneration()) reportError(error)
    return null
  }
}

const historyNavigationListeners = new Set<() => void>()

/**
 * Runs before Undo, Redo or a history jump starts its backend request, even
 * when navigation turns out to be a no-op. Transient UI choices can cancel
 * without waiting for a patch. This signal never changes document state.
 */
export function onHistoryNavigation(listener: () => void): () => void {
  historyNavigationListeners.add(listener)
  return () => {
    historyNavigationListeners.delete(listener)
  }
}

async function runHistory(work: () => Promise<ProjectPatch | null>) {
  const generation = getProjectGeneration()
  for (const listener of [...historyNavigationListeners]) listener()
  try {
    const patch = await work()
    if (patch && generation === getProjectGeneration()) receivePatch(patch)
  } catch (error) {
    if (generation === getProjectGeneration()) reportError(error)
  }
}

export function undo(): Promise<void> {
  return runHistory(() => backend.undo())
}

export function redo(): Promise<void> {
  return runHistory(() => backend.redo())
}

/** Undoes or redoes until `cursor` history entries are applied. */
export function historyJump(cursor: number): Promise<void> {
  return runHistory(() => backend.historyJump(cursor))
}

/**
 * Records where a save went, which travels outside the patch stream.
 * Whether the project is clean is not decided here: the shell sends a patch
 * with `dirty` after every save, and it knows about an edit that came in
 * while the file was being written.
 */
export function setProjectPath(path: string) {
  useProjectStore.setState({ path })
}
