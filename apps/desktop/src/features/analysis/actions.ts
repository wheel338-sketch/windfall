import { create } from "zustand"
import { invalidateActionsOn, registry, type Action } from "@/lib/actions"
import { useProjectStore } from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { useUiStore } from "@/lib/store/ui"
import { usePlaylistStore } from "@/features/playlist/store"
import { useRecordingStore } from "@/features/transport/recording-store"
import { loadRemixSong, useRemixLoader } from "./remix"
import {
  hasRecoveryCapacity,
  recoverJob,
  selectedRecoveryJob,
  selectNextRecoveryJob,
  useAnalysisRecovery,
} from "./recovery"
import {
  isTerminalStatus,
  validRange,
  taskProblem,
  isDereverb,
  isBuiltinDereverb,
  BUILTIN_DEREVERB_ID,
  type AnalysisCapability,
  type AnalysisJob,
  type AnalysisReview,
  type AnalysisTask,
} from "./types"
import { translatedText as t, message, dynamicTitle } from "@/lib/i18n"

/** UI identity only. Native capture/recheck remains the installation authority. */
export function captureAnalysisClip(id: number) {
  const selection = usePlaylistStore.getState().selection
  if (selection.size !== 1 || !selection.has(id)) return null
  const document = useProjectStore.getState()
  const clip = document.project.playlist.clips.find((clip) => clip.id === id)
  if (!clip || clip.content.type !== "audio") return null
  const sampleId = clip.content.sample
  const source = document.project.samples.find(
    (sample) => sample.id === sampleId
  )
  if (!source) return null
  const generation = getProjectGeneration()
  const revision = document.revision
  const path = document.path
  return {
    id,
    generation,
    current: () => {
      const next = useProjectStore.getState()
      return (
        generation === getProjectGeneration() &&
        revision === next.revision &&
        path === next.path &&
        selection === usePlaylistStore.getState().selection &&
        next.project.playlist.clips.find((item) => item.id === id) === clip &&
        next.project.samples.find((item) => item.id === sampleId) === source
      )
    },
  }
}
type Capture = NonNullable<ReturnType<typeof captureAnalysisClip>>

/** One dialog target and one mounted panel; no retained job map or audio. */
export const useAnalysisDialog = create(() => ({
  target: null as (Capture & { task?: AnalysisTask }) | null,
}))
export function closeAnalysis() {
  useAnalysisDialog.setState({ target: null })
}

export type AnalysisCommand =
  | "chooseModel"
  | "importModel"
  | "submit"
  | "cancel"
  | "review"
  | "retryCleanup"
  | "forget"
  | "apply"
  | "cancelPreparation"

type PanelContext = {
  clip: number
  current(): boolean
  stale: boolean
  busy: boolean
  capability: AnalysisCapability | null
  job: AnalysisJob | null
  review: AnalysisReview | null
  modelIndex: number
  start: string
  end: string
  manifest: string
  localPath: string
  replace: boolean
  task?: AnalysisTask
  amount?: number
  selectedRoles?: readonly string[]
  execute(command: AnalysisCommand): Promise<void>
}
let panel: PanelContext | null = null

/** The disposer can remove only its own context, never a replacement panel. */
export function bindAnalysisPanel(context: PanelContext): () => void {
  panel = context
  registry.invalidate()
  return () => {
    if (panel === context) {
      panel = null
      registry.invalidate()
    }
  }
}

function recording(): boolean {
  const state = useRecordingStore.getState()
  return state.busy || state.state.active
}
function selectedClip() {
  const selection = usePlaylistStore.getState().selection
  if (selection.size !== 1) return null
  const id = selection.values().next().value
  return id === undefined ? null : captureAnalysisClip(id)
}

function reason(command: AnalysisCommand): string | undefined {
  const context = panel
  if (!context) return t("Open Analysis for an audio clip")
  if (command === "cancelPreparation")
    return context.busy ? undefined : t("No analysis preparation is active")
  if (context.busy) return t("Analysis operation in progress")
  // Job retirement does not change the document and remains possible when stale.
  if (["cancel", "retryCleanup", "forget"].includes(command)) {
    if (!context.job) return t("No retained analysis job")
    if (command === "cancel" && isTerminalStatus(context.job.status))
      return t("Job is already retired")
    if (command === "forget" && !isTerminalStatus(context.job.status))
      return t("Cancel the job before forgetting it")
    return undefined
  }
  if (context.stale || !context.current())
    return t("Project, clip selection or source changed; reopen Analysis")
  if (recording()) return t("Finish recording before analysis preparation")
  if (!context.capability) return t("Checking native availability")
  if (!context.capability.native)
    return context.capability.reason ?? t("Analysis requires the desktop app")
  if (command === "chooseModel") return undefined
  if (command === "importModel")
    return context.localPath && context.manifest
      ? undefined
      : t("Choose a local file and enter its pinned manifest")
  if (!context.capability.available)
    return context.capability.reason ?? t("No inference algorithm is available")
  const model = context.capability.models[context.modelIndex]
  const builtin = context.capability.classicalDereverb && isDereverb(context.task ?? "audio") &&
    (context.modelIndex === -1 || context.capability.models.length === 0)
  if (command === "submit") {
    if (!hasRecoveryCapacity())
      return t("Retire retained analysis jobs first (limit 8)")
    if (context.job) return t("Retire the current job before submitting another")
    if (!model && !builtin) return t("Import and select a pinned model")
    const amount = context.amount ?? 1
    if (!Number.isFinite(amount) || amount < 0 || amount > 1) return t("Wet/dry amount must be 0 to 1")
    const mismatch = !builtin && model ? taskProblem(context.task ?? "audio", model) : undefined
    if (mismatch) return mismatch
    if (context.task === "stemSeparation" && context.capability.stemSeparation === false)
      return context.capability.reason ?? t("Choose an installed ONNX Runtime library in Settings")
    if (!builtin && model && context.capability.classicalDereverb && !context.capability.stemSeparation && model.provenance.adapterId !== BUILTIN_DEREVERB_ID)
      return t("Pack registered; the ONNX Runtime separation adapter is not installed")
    if (!validRange(context.start, context.end))
      return t("Enter a valid frame range")
  }
  if (command === "review" && context.job?.status !== "ready")
    return t("Job outputs are not ready")
  if (command === "apply") {
    const { job, review, capability } = context
    if (!review || job?.status !== "ready") return t("Review ready outputs first")
    if (context.selectedRoles?.length === 0) return t("Select at least one reviewed output")
    const modelCurrent = (capability.classicalDereverb && isBuiltinDereverb(review.model)) ||
      capability.models.some((model) => model.id === review.model.id &&
        model.version === review.model.version && model.revision === review.model.revision)
    if (
      review.job.job !== job.job ||
      review.job.ticket !== job.ticket ||
      review.job.request !== job.request ||
      review.clip !== context.clip ||
      !modelCurrent
    )
      return t("Review or model revision changed; review again")
    if (
      context.replace &&
      (review.startFrame !== "0" || review.endFrame !== review.inputFrames)
    )
      return t("Replacement requires the complete input range")
  }
  return undefined
}

function command(id: AnalysisCommand, title: string): Action {
  return {
    id: `analysis.${id}`,
    title,
    section: "Analysis",
    enabled: () => reason(id) === undefined,
    whyDisabled: () => reason(id),
    run: async () => {
      // Recheck for callers that kept metadata from an earlier menu lifetime.
      if (reason(id) === undefined) await panel?.execute(id)
    },
  }
}
function recoveryCommand(
  id: string,
  title: string,
  operation: "cancel" | "retry" | "forget"
): Action {
  const reason = () => {
    const item = selectedRecoveryJob()
    if (!item) return t("No selected retained analysis job")
    if (item.busy) return message("Analysis job {job}: cleanup in progress", { job: item.job.job })
    if (operation === "cancel" && isTerminalStatus(item.job.status))
      return message("Analysis job {job} is retired", { job: item.job.job })
    if (operation === "forget" && !isTerminalStatus(item.job.status))
      return message("Cancel analysis job {job} first", { job: item.job.job })
    return undefined
  }
  return {
    id: `analysis.${id}`,
    title,
    section: "Analysis",
    enabled: () => reason() === undefined,
    whyDisabled: reason,
    run: async () => {
      if (reason() === undefined) await recoverJob(operation)
    },
  }
}
export const ANALYSIS_ACTIONS: Action[] = [
  {
    id: "analysis.open",
    ...dynamicTitle("Analysis", {}),
    section: "Analysis",
    defaultShortcut: "Mod+Shift+A",
    keywords: "audio clip native model review",
    enabled: () =>
      selectedClip() !== null &&
      !recording() &&
      useAnalysisDialog.getState().target === null,
    whyDisabled: () =>
      useAnalysisDialog.getState().target !== null
        ? t("Analysis is already open")
        : recording()
          ? t("Finish recording first")
          : t("Select one audio clip with a source"),
    run: () => {
      if (recording() || useAnalysisDialog.getState().target !== null) return
      const target = selectedClip()
      if (!target) return
      useUiStore.getState().showCenterTab("playlist")
      usePlaylistStore.setState({ inspectorOpen: true })
      useAnalysisDialog.setState({ target })
    },
  },
  command("chooseModel", "Choose local model"),
  command("importModel", "Verify and import local model"),
  command("submit", "Submit analysis"),
  command("cancel", "Cancel job"),
  command("review", "Review outputs"),
  command("retryCleanup", "Retry owned cleanup"),
  command("forget", "Forget retired job"),
  command("apply", "Apply reviewed outputs"),
  command("cancelPreparation", "Cancel active analysis preparation"),
  recoveryCommand("recoveryCancel", "Cancel retained job", "cancel"),
  recoveryCommand("recoveryRetry", "Retry retained job cleanup", "retry"),
  recoveryCommand("recoveryForget", "Forget retained job", "forget"),
  {
    id: "analysis.recoveryNext",
    ...dynamicTitle("Select next retained analysis job", {}),
    section: "Analysis",
    enabled: () =>
      useAnalysisRecovery.getState().jobs.some((item) => item.dismissed),
    whyDisabled: () => t("No retained analysis job"),
    run: selectNextRecoveryJob,
  },
  {
    id: "analysis.remix",
    ...dynamicTitle("Remix a song", {}),
    section: "Analysis",
    keywords: "load song stems drums bass vocals separate playlist mixer",
    enabled: () => !recording() && !useRemixLoader.getState().busy &&
      useAnalysisDialog.getState().target === null,
    whyDisabled: () => recording() ? t("Finish recording first") :
      useRemixLoader.getState().busy ? t("Song import in progress") : t("Close Analysis first"),
    run: async () => {
      const current = () => !recording() && useAnalysisDialog.getState().target === null
      const id = await loadRemixSong(current)
      if (id === null || !current()) return
      const target = captureAnalysisClip(id)
      if (!target) return
      useUiStore.getState().showCenterTab("playlist")
      usePlaylistStore.setState({ inspectorOpen: true })
      useAnalysisDialog.setState({ target: { ...target, task: "stemSeparation" } })
    },
  },
  {
    id: "analysis.separate",
    ...dynamicTitle("Separate into four stems", {}),
    section: "Analysis",
    keywords: "drums bass vocals other separate clip",
    enabled: () => selectedClip() !== null && !recording() && useAnalysisDialog.getState().target === null,
    whyDisabled: () => recording() ? t("Finish recording first") : t("Select one audio clip and close Analysis"),
    run: () => { const target = selectedClip(); if (target && !recording()) openStemClip(target.id) },
  },
]

export function openStemClip(id: number) {
  if (recording() || useAnalysisDialog.getState().target !== null) return
  const target = captureAnalysisClip(id)
  if (!target) return
  useUiStore.getState().showCenterTab("playlist")
  usePlaylistStore.setState({ inspectorOpen: true })
  useAnalysisDialog.setState({ target: { ...target, task: "stemSeparation" } })
}

/** Startup registration and state invalidation share the app registry lifecycle. */
export function registerAnalysisActions(): () => void {
  const stops = [
    registry.register(ANALYSIS_ACTIONS),
    invalidateActionsOn(useAnalysisDialog, (state) => [state.target]),
    invalidateActionsOn(useRemixLoader, (state) => [state.busy]),
    invalidateActionsOn(useAnalysisRecovery, (state) => [
      state.jobs,
      state.pending,
      state.selected,
    ]),
    invalidateActionsOn(usePlaylistStore, (state) => [state.selection]),
    invalidateActionsOn(useRecordingStore, (state) => [
      state.busy,
      state.state.active,
    ]),
    useProjectStore.subscribe(() => registry.invalidate()),
    onProjectReplaced(() => {
      panel = null
      useRemixLoader.setState({ clip: null, tempoCandidates: [], tempoProblem: null })
      closeAnalysis()
      registry.invalidate()
    }),
  ]
  return () => {
    for (const stop of stops) stop()
    panel = null
    closeAnalysis()
    registry.invalidate()
  }
}
