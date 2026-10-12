/** Native process-volatile protocol. u64 identifiers/counts stay decimal strings. */
export type AnalysisProvenance = {
  origin: string
  sourceRevision: string
  author: string
  licenseSpdx: string
  licenseReference: string
  adapterId: string
  adapterVersion: string
  device: string
}
export type AnalysisTensorSpec = { name: string; shape: (number | string)[] }
export type AnalysisOnnxContract = {
  opset: number; inputs: AnalysisTensorSpec[]; outputs: AnalysisTensorSpec[]
  sampleRate: number; segmentFrames: number; overlapFrames: number; stemRoles: string[]
  layout?: "channelsFirst" | "channelsLast"
  contextFrames?: number
}
export type AnalysisRuntimeSettings = {
  libraryPath: string | null
  modelId: string | null
  modelVersion: string | null
  packDirectory?: string | null
}
export type AnalysisModel = {
  onnx?: AnalysisOnnxContract | null
  id: string
  version: string
  revision: string
  sha256: string
  bytes: string
  maxBytes: string
  provenance: AnalysisProvenance
  sampleRate: number
  inputChannels: number
  maxInputFrames: string
  outputs: { role: string; channels: number }[]
}
export type AnalysisCapability = {
  stemSeparation?: boolean
  classicalDereverb?: boolean
  native: boolean
  available: boolean
  reason: string | null
  models: AnalysisModel[]
}
export type AnalysisSubmit = {
  amount?: number | null
  task?: AnalysisTask
  clip: number
  modelId: string
  modelVersion: string
  modelRevision: string
  startFrame: string
  endFrame: string
}
export type AnalysisStatus =
  | "queued"
  | "running"
  | "cancelling"
  | "cancelled"
  | "failed"
  | "ready"
  | "consumed"
export function isTerminalStatus(status: AnalysisStatus): boolean {
  return status === "cancelled" || status === "failed" || status === "consumed"
}
export type AnalysisJob = {
  job: string
  ticket: string
  request: string
  sequence: string
  status: AnalysisStatus
  completedWork: string
  maximumWork: string
  failure: string | null
}
export type AnalysisReview = {
  amount: number
  task?: AnalysisTask
  job: AnalysisJob
  clip: number
  generation: string
  editRevision: string
  sourceSha256: string
  bindingSha256: string
  startFrame: string
  endFrame: string
  inputFrames: string
  model: AnalysisModel
  artifacts: {
    name: string
    role: string
    frames: string
    channels: number
    sampleRate: number
    frameOrigin: string
    bytes: string
    sha256: string
  }[]
}
export type AnalysisApply = {
  ticket: string
  request: string
  replaceOriginal: boolean
  selectedRoles?: string[] | null
  separateTracks?: boolean
  muteOriginal?: boolean
}
export type AnalysisTask = "audio" | "stemSeparation" | "dereverbGeneral" | "dereverbVocal"
export const BUILTIN_DEREVERB_ID = "windfall-classical-dereverb"
export function isDereverb(task: AnalysisTask): boolean {
  return task === "dereverbGeneral" || task === "dereverbVocal"
}
export function isBuiltinDereverb(model: AnalysisModel): boolean {
  return model.id === BUILTIN_DEREVERB_ID && model.version === "1" && model.revision === "1" && model.bytes === "0"
}
export function taskProblem(task: AnalysisTask, model: AnalysisModel): string | undefined {
  if (task === "audio") return undefined
  const expected = task === "stemSeparation" ? ["drums", "bass", "vocals", "other"] :
    [task === "dereverbGeneral" ? "dereverb-general" : "dereverb-vocal"]
  const roles = model.outputs.map((o) => o.role)
  if (roles.length !== expected.length || new Set(roles).size !== roles.length ||
    expected.some((role) => !roles.includes(role)) ||
    model.outputs.some((o) => o.channels !== model.inputChannels))
    return "Select a pinned model with the exact roles and preserved channels for this operation."
  return undefined
}
export interface AnalysisBackend {
  analysisRuntimeSettings(): Promise<AnalysisRuntimeSettings>
  analysisRuntimeConfigure(settings: AnalysisRuntimeSettings): Promise<AnalysisRuntimeSettings>
  pickAnalysisRuntime(): Promise<string | null>
  analysisCapability(): Promise<AnalysisCapability>
  analysisModelImport(
    path: string,
    model: AnalysisModel
  ): Promise<AnalysisModel>
  pickAnalysisModel(): Promise<string | null>
  analysisSubmit(request: AnalysisSubmit): Promise<AnalysisJob>
  analysisStatus(job: string): Promise<AnalysisJob>
  analysisCancel(job: string): Promise<AnalysisJob>
  analysisCancelPreparation(): Promise<void>
  analysisForget(job: string): Promise<void>
  analysisRetryCleanup(job: string): Promise<void>
  analysisReview(ticket: string): Promise<AnalysisReview>
  analysisPreview(ticket: string, role: string): Promise<void>
  analysisApply(
    request: AnalysisApply
  ): Promise<import("@/bindings").DispatchResult>
  analysisShutdown(): Promise<void>
}
export function decimal(value: string): boolean {
  return (
    /^(0|[1-9][0-9]{0,19})$/.test(value) &&
    BigInt(value) <= 18446744073709551615n
  )
}
export function validRange(start: string, end: string): boolean {
  return decimal(start) && decimal(end) && BigInt(start) < BigInt(end)
}
