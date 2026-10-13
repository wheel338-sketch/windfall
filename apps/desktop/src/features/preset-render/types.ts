import type { ChannelId, DispatchResult, LibraryFileToken, SampleId } from "@/bindings"

/** Runtime IPC types; the integrator regenerates persisted Rust bindings. */
export type PresetRenderSettings = {
  note: number
  velocity: number
  lengthSecs: number
  tailSecs: number
  stereo: boolean
}
export type PresetRenderTicket = { request: number; sampleRate: number }
export type PresetRenderInput = {
  path: string
  source?: SampleId
  channel?: ChannelId
  index?: number
  browser?: LibraryFileToken
  options: PresetRenderSettings
}
export type SamplePresetRender = { sample: SampleId; source: SampleId; settings: PresetRenderSettings }
export type PresetRenderBackend = {
  presetRenderBegin(): Promise<PresetRenderTicket>
  presetRenderCancel(request: number): Promise<void>
  presetRenderFile(request: number, input: PresetRenderInput): Promise<DispatchResult>
}
export const DEFAULT_RENDER: PresetRenderSettings = {
  note: 60, velocity: 0.8, lengthSecs: 1, tailSecs: 3, stereo: true,
}
export const UNSUPPORTED_PRESET = "This proprietary preset source is unsupported: no public file specification is available. Use a Windfall synthesizer .wfpreset or save a Text Voice channel .wfpreset, then choose Render as sample."
export function isPresetPath(path: string) { return /\.(wfpreset|syn|speech)$/i.test(path) }
export function isProprietaryPreset(path: string) { return /\.(syn|speech)$/i.test(path) }
