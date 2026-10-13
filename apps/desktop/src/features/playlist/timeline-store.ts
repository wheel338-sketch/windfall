import { create } from "zustand"
import type {
  MarkerKind,
  MeterChange,
  TickRange,
  TimelineMarker,
  TimelinePlaybackState,
} from "@/bindings"
import { backend } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { translatedText as t } from "@/lib/i18n"

export type RulerTool = "seek" | "select" | "zoom"
export type TimelineEdit =
  | { type: "meter"; item?: MeterChange }
  | { type: "marker"; kind: MarkerKind; item?: TimelineMarker }
export const useTimelineStore = create(() => ({
  tool: "seek" as RulerTool,
  selection: null as TickRange | null,
  draft: null as TickRange | null,
  active: false,
  playback: null as TimelinePlaybackState | null,
  hydrated: false,
  exportSelection: false,
  error: null as string | null,
  selected: null as { type: "meter" | "marker"; id: number } | null,
  edit: null as TimelineEdit | null,
}))
let currentRequest: TimelineRequest | null = null
let wireRequest = 0
const pending = new Set<TimelineRequest>()
type TimelineRequest = {
  generation: number
  revision: number
  playPending: boolean
  guard: TimelinePlaybackState | null
  cancel: TimelinePlaybackState | null
  intentCurrent(): boolean
  current(): boolean
}
export function beginTimelineRequest(playPending = false): TimelineRequest {
  const generation = getProjectGeneration()
  const revision = useProjectStore.getState().revision
  const cancelled = currentRequest?.playPending
    ? (currentRequest.guard ?? currentRequest.cancel)
    : currentRequest?.cancel
  const operation: TimelineRequest = {
    generation,
    playPending,
    guard: null,
    cancel: cancelled ?? null,
    intentCurrent: () =>
      currentRequest === operation && generation === getProjectGeneration(),
    current: () =>
      operation.intentCurrent() &&
      revision === useProjectStore.getState().revision,
    revision,
  }
  currentRequest = operation
  return operation
}
onProjectReplaced(() => {
  currentRequest = null
  useTimelineStore.setState({
    tool: "seek",
    selection: null,
    draft: null,
    active: false,
    playback: null,
    hydrated: false,
    exportSelection: false,
    error: null,
    selected: null,
    edit: null,
  })
})
useProjectStore.subscribe(({ project }) => {
  const selected = useTimelineStore.getState().selected
  const items =
    selected?.type === "meter"
      ? project.playlist.timeline?.meters
      : project.playlist.timeline?.markers
  if (selected && !items?.some((item) => item.id === selected.id))
    useTimelineStore.setState({ selected: null, edit: null })
})

export function timelineRequestError(
  operation: TimelineRequest,
  error: unknown
) {
  if (operation.current())
    useTimelineStore.setState({
      error: error instanceof Error ? error.message : String(error),
    })
}

function receiveTimelineState(
  state: TimelinePlaybackState,
  restoreSelection: boolean
) {
  const prior = useTimelineStore.getState().playback
  if (
    prior &&
    prior.generation === state.generation &&
    prior.request > state.request
  )
    return
  useTimelineStore.setState({
    playback: state,
    hydrated: true,
    active: state.region !== null,
    ...(restoreSelection ? { selection: state.region } : {}),
  })
}

/** Hydrate retained native regions on playlist mount; never override a newer intent. */
export async function refreshTimelineState() {
  const generation = getProjectGeneration()
  const revision = useProjectStore.getState().revision
  const intent = currentRequest
  try {
    const state = await backend.timelineState()
    if (
      generation !== getProjectGeneration() ||
      intent !== currentRequest ||
      revision !== useProjectStore.getState().revision ||
      revision !== state.revision
    )
      return
    receiveTimelineState(state, state.region !== null)
  } catch (error) {
    if (generation === getProjectGeneration() && intent === currentRequest)
      useTimelineStore.setState({
        error: error instanceof Error ? error.message : String(error),
      })
  }
}

async function recoverTimelineState(
  operation: TimelineRequest,
  error: unknown
) {
  if (!operation.intentCurrent()) return
  // Queries/IO stay outside native State. A failed mutation leaves canonical
  // region/playback untouched and visible, including an edit-invalidated reply.
  try {
    const state = await backend.timelineState()
    if (!operation.intentCurrent()) return
    receiveTimelineState(state, true)
  } catch {
    // Retain the last canonical region and retry affordance if recovery fails.
  } finally {
    if (operation.intentCurrent())
      useTimelineStore.setState({
        error: error instanceof Error ? error.message : String(error),
      })
  }
}

export function finishTimelinePlay(operation: TimelineRequest) {
  if (operation.current()) operation.playPending = false
}

/** Native request ordering reconciles publications even when their replies arrive late. */
export async function publishPlaybackRegion(
  region: TickRange | null,
  operation: TimelineRequest,
  selectionOnly = false
): Promise<TimelinePlaybackState | null> {
  const hadPendingPublication = pending.size > 0
  pending.add(operation)
  try {
    const state = await backend.timelineState()
    if (!operation.current() || operation.revision !== state.revision)
      throw new Error(
        t("The project changed before the timeline selection could be applied. Retry the selection or Clear.")
      )
    receiveTimelineState(state, state.region !== null)
    if (
      selectionOnly &&
      region !== null &&
      state.region === null &&
      !hadPendingPublication &&
      !operation.cancel
    ) {
      useTimelineStore.setState({ selection: region, error: null })
      return state
    }
    if (!Number.isSafeInteger(state.request) || state.request < 0)
      throw new Error(
        t("The backend returned an invalid timeline request number.")
      )
    wireRequest = Math.max(wireRequest, state.request)
    if (wireRequest >= Number.MAX_SAFE_INTEGER)
      throw new Error(
        t("The timeline request limit was reached. Restart the app before selecting another playback region.")
      )
    const request = ++wireRequest
    // An edit invalidates the source revision, not an unchanged action owner.
    // A retry uses this fresh canonical revision while retaining the exact
    // generation/request/range of the action being cancelled.
    const cancel = operation.cancel
      ? { ...operation.cancel, revision: state.revision }
      : undefined
    const published = await backend.timelineRegion(
      region,
      state.generation,
      state.revision,
      request,
      cancel
    )
    if (!operation.current())
      throw new Error(
        t("The project changed while the timeline selection was being applied. Retry the selection or Clear.")
      )
    receiveTimelineState(published, true)
    // An unpublished successor cannot reach guarded transport. Until its
    // accepted reply, keep the inherited live cancellation target instead.
    if (operation.playPending) operation.guard = published
    operation.cancel = null
    useTimelineStore.setState({ error: null })
    return published
  } catch (error) {
    await recoverTimelineState(operation, error)
    return null
  } finally {
    pending.delete(operation)
  }
}

export async function applyPlaybackRegion(region: TickRange | null) {
  return (await publishPlaybackRegion(region, beginTimelineRequest())) !== null
}

export async function selectTimelineRegion(region: TickRange | null) {
  const operation = beginTimelineRequest()
  // Keep a canonical active range visible until native accepts its replacement.
  if (!useTimelineStore.getState().active)
    useTimelineStore.setState({ selection: region })
  useTimelineStore.setState({ error: null })
  await publishPlaybackRegion(region, operation, true)
}

export function editTimeline(edit: TimelineEdit) {
  useTimelineStore.setState({
    edit,
    selected: edit.item ? { type: edit.type, id: edit.item.id } : null,
  })
}
