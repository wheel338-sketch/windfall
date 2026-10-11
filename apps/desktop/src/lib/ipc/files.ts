import type { Channel, ChannelId, DispatchResult, DocumentSnapshot, PatternId, PluginAnnotation, PluginDatabaseView, PluginTarget, PresetCapture, PresetEntry, PresetReview, Score, ScoreLoadMode, ScoreReview, ScoreWritten, TemplateInfo } from "@/bindings"

/** Disk-backed file workflows. Browser capabilities fail explicitly when disk or native state is required. */
export interface ProjectFilesBackend {
  presetCaptureChannel(id: ChannelId, generation: number, revision: number): Promise<PresetCapture>
  presetCapturePlugin(target: PluginTarget, generation: number, revision: number): Promise<PresetCapture>
  presetWrite(path: string, capture: PresetCapture): Promise<string>
  presetRead(path: string): Promise<PresetReview>
  presetApplyChannel(path: string, id: ChannelId, expected: Channel, nameColor: boolean, withoutAudio: boolean): Promise<DispatchResult>
  presetLibrary(): Promise<PresetEntry[]>
  presetLibrarySave(name: string, capture: PresetCapture): Promise<PresetEntry>
  presetLibraryDelete(path: string): Promise<void>
  scoreCapture(pattern: PatternId, channel: ChannelId, only: number[] | null, generation: number, revision: number): Promise<Score>
  scoreApply(score: Score, pattern: PatternId, channel: ChannelId, mode: ScoreLoadMode, generation: number, revision: number): Promise<DispatchResult>
  scoreWrite(path: string, score: Score): Promise<ScoreWritten>
  scoreRead(path: string): Promise<ScoreReview>
  templates(): Promise<TemplateInfo[]>
  templateSave(name: string): Promise<TemplateInfo>
  templateNew(id: string): Promise<DocumentSnapshot>
  templateDelete(id: string): Promise<void>
  pluginDatabase(): Promise<PluginDatabaseView>
  pluginDatabaseAnnotate(format: string, id: string, annotation: PluginAnnotation): Promise<PluginDatabaseView>
  pluginDatabaseForgetMissing(): Promise<PluginDatabaseView>
  pluginsRemoveFolder(folder: string): Promise<void>
  pluginsRescan(everything: boolean): Promise<void>
  pickPresetPath(saveName?: string): Promise<string | null>
  pickScorePath(saveName?: string): Promise<string | null>
}

export const FILES_NATIVE_ONLY = "Preset files, native state and portable templates require the desktop app."
