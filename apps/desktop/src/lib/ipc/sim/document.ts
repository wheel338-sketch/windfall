import type {
  Command,
  DispatchResult,
  DocumentSnapshot,
  Pattern,
  Project,
  ProjectPatch,
  MidiImportOptions,
  MidiExportOptions,
  MidiImportPreview,
} from "@/bindings"

import { sim } from "./wasm"

// Frees the document of a `SimDocument` that was dropped without `dispose`,
// so a forgotten mock costs no WebAssembly memory once it is collected.
const forgotten = new FinalizationRegistry<number>((handle) => {
  try {
    sim.call("doc_free", handle)
  } catch {
    // Only a module that has crashed refuses, and then nothing is left to free.
  }
})

function mergePatterns(current: Pattern[], patch: ProjectPatch): Pattern[] {
  const byId = new Map(current.map((pattern) => [pattern.id, pattern]))
  for (const pattern of patch.patterns ?? []) byId.set(pattern.id, pattern)
  const order = patch.patternOrder ?? current.map((pattern) => pattern.id)
  return order.flatMap((id) => byId.get(id) ?? [])
}

/**
 * One `windfall_project::Document`, living in the WebAssembly module: the
 * project and its undo history, with every rule the app has.
 *
 * The project is also kept here as a plain object, brought up to date from
 * the patches the document produces, for the parts of the mock that read it
 * many times a second.
 */
export class SimDocument {
  private handle: number | null
  private mirror: Project
  private dirty: boolean

  private constructor(handle: number) {
    this.handle = handle
    forgotten.register(this, handle, this)
    const snapshot = this.snapshot(null)
    this.mirror = snapshot.project
    this.dirty = snapshot.dirty
  }

  /** A document on a project that counts as saved. Throws if it breaks a rule. */
  static create(project: Project): SimDocument {
    return new SimDocument(sim.call<number>("doc_new", 0, project))
  }

  /** A checked imported project which has not been saved. */
  static imported(project: Project): SimDocument {
    return new SimDocument(sim.call<number>("doc_new_unsaved", 0, project))
  }

  /** A document on the text of a `.windfall` file, loaded as the app loads one. */
  static open(fileText: string): SimDocument {
    return new SimDocument(sim.call<number>("doc_from_file_json", 0, fileText))
  }

  /** Closes the document. Calling it again does nothing. */
  dispose() {
    if (this.handle === null) return
    forgotten.unregister(this)
    const handle = this.handle
    this.handle = null
    sim.call("doc_free", handle)
  }

  project(): Project {
    return this.mirror
  }

  isDirty(): boolean {
    return this.dirty
  }

  /**
   * Applies a command. Dispatches in a row that carry the same gesture id
   * are one undo step.
   */
  dispatch(command: Command, gesture?: number): DispatchResult {
    const result = this.call<DispatchResult>("doc_dispatch", {
      command,
      gesture,
    })
    this.follow(result.patch)
    // A patch does not carry the id counter, and only a dispatch moves it.
    this.mirror.nextId = this.call<number>("doc_next_id")
    return result
  }

  undo(): ProjectPatch | null {
    return this.follow(this.call<ProjectPatch | null>("doc_undo"))
  }

  assistant(prompt: string, generation: number): import("@/bindings").AssistantReply {
    return this.call("doc_assistant", { prompt, generation })
  }

  applyProposal(proposal: import("@/bindings").CommandProposal, generation: number, recording: boolean): DispatchResult {
    const result = this.call<DispatchResult>("doc_proposal_apply", { proposal, generation, recording })
    this.follow(result.patch)
    this.mirror.nextId = this.call<number>("doc_next_id")
    return result
  }

  importMidi(bytes: number[], options: MidiImportOptions): DispatchResult {
    const result = this.call<DispatchResult>("doc_midi_import", {
      bytes,
      options,
    })
    this.follow(result.patch)
    this.mirror.nextId = this.call<number>("doc_next_id")
    return result
  }

  previewMidi(bytes: number[], options: MidiImportOptions): MidiImportPreview {
    return this.call<MidiImportPreview>("midi_preview", { bytes, options })
  }

  exportMidi(options: MidiExportOptions): number[] {
    return this.call<number[]>("doc_midi_export", options)
  }

  capturePreset(channel: number | null, target: import("@/bindings").PluginTarget | null): import("@/bindings").PresetCapture {
    return this.call("doc_preset_capture", { channel, target })
  }
  presetCommand(preset: import("@/bindings").Preset, channel: number | null, expected: import("@/bindings").Channel | null, target: import("@/bindings").PluginTarget | null, nameColor = false): Command {
    return this.call("doc_preset_command", { preset, channel, expected, target, name_color: nameColor })
  }
  captureScore(pattern: number, channel: number, only: number[] | null): import("@/bindings").Score {
    return this.call("doc_score_capture", { pattern, channel, only })
  }
  scoreCommand(score: import("@/bindings").Score, pattern: number, channel: number, mode: import("@/bindings").ScoreLoadMode): Command {
    return this.call("doc_score_command", { score, pattern, channel, mode })
  }
  writeScore(score: import("@/bindings").Score, path: string): { bytes: number[]; notes: string[] } {
    return this.call("doc_score_write", { score, path })
  }
  readScore(bytes: number[], name: string): import("@/bindings").ScoreReview {
    return this.call("score_read", { bytes, name })
  }

  redo(): ProjectPatch | null {
    return this.follow(this.call<ProjectPatch | null>("doc_redo"))
  }

  /** Undoes or redoes until `cursor` history entries are applied. */
  jump(cursor: number): ProjectPatch {
    return this.follow(this.call<ProjectPatch>("doc_jump", cursor))
  }

  /** Records a save. The patch changes nothing but the dirty flag. */
  markSaved(): ProjectPatch {
    return this.follow(this.call<ProjectPatch>("doc_mark_saved"))
  }

  snapshot(path: string | null): DocumentSnapshot {
    return this.call<DocumentSnapshot>("doc_snapshot", path)
  }

  /** The text of the `.windfall` file a save writes. */
  fileText(): string {
    return this.call<string>("doc_to_file_json")
  }

  private call<T>(operation: string, input?: unknown): T {
    if (this.handle === null) throw new Error("This document was closed.")
    return sim.call<T>(operation, this.handle, input)
  }

  private follow<T extends ProjectPatch | null>(patch: T): T {
    if (patch === null) return patch
    const project = this.mirror
    this.mirror = {
      ...project,
      plugins: patch.plugins ?? project.plugins,
      settings: patch.settings ?? project.settings,
      notebook: patch.notebook ?? project.notebook,
      samples: patch.samples ?? project.samples,
      channels: patch.channels ?? project.channels,
      mixer: patch.mixer ?? project.mixer,
      playlist: patch.playlist ?? project.playlist,
      automations: patch.automations ?? project.automations,
      patterns: mergePatterns(project.patterns, patch),
    }
    if (!this.mirror.plugins?.length) delete this.mirror.plugins
    if (!this.mirror.notebook?.pages.length) delete this.mirror.notebook
    this.dirty = patch.dirty
    return patch
  }
}
