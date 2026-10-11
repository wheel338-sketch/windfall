import { toast } from "sonner"
import type { PresetCapture } from "@/bindings"
import { registry } from "@/lib/actions"
import { backend } from "@/lib/ipc"
import { reportError } from "@/lib/errors"
import { receivePatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { useUiStore } from "@/lib/store/ui"
import { askConfirm, askText } from "@/lib/store/prompts"
import { confirmDiscardChanges } from "@/lib/flows/project"
import { currentPatternId } from "@/lib/flows/edit"
import { currentSession } from "@/features/piano-roll/session"
import { selectedEffect } from "@/features/mixer/effect-ops"

function stamp() { return { generation: getProjectGeneration(), revision: useProjectStore.getState().revision } }
function current(ticket: ReturnType<typeof stamp>) { if (ticket.generation !== getProjectGeneration() || ticket.revision !== useProjectStore.getState().revision) throw new Error("The project changed while the file was chosen; try again") }
export async function captureSelectedPreset(plugin = false): Promise<PresetCapture> {
  const ticket = stamp()
  const state = await backend.timelineState()
  current(ticket)
  const effect = plugin ? selectedEffect() : undefined
  const channel = useUiStore.getState().selectedChannel
  const captured = effect ? await backend.presetCapturePlugin({ type: "effect", effect: effect.slot.id }, state.generation, state.revision) : channel !== null ? plugin ? await backend.presetCapturePlugin({ type: "instrument", channel }, state.generation, state.revision) : await backend.presetCaptureChannel(channel, state.generation, state.revision) : null
  current(ticket)
  if (!captured) throw new Error("Select a channel or plugin first")
  return captured
}
export async function saveSelectedPreset(plugin = false, library = false) {
  const capture = await captureSelectedPreset(plugin)
  if (library) {
    const name = await askText({ title: "Save preset to library", label: "Name", initial: capture.preset.name, submitLabel: "Save preset" })
    if (name !== null) await backend.presetLibrarySave(name, capture)
  } else {
    const path = await backend.pickPresetPath(capture.preset.name)
    if (path !== null) await backend.presetWrite(path, capture)
  }
}
export async function applyPresetFile(chosen?: string) {
  const ticket = stamp()
  const channel = useProjectStore.getState().project.channels.find((c) => c.id === useUiStore.getState().selectedChannel)
  const state = await backend.timelineState(); current(ticket)
  const effect = selectedEffect()
  const target = effect ? { type: "effect" as const, effect: effect.slot.id } : channel ? { type: "instrument" as const, channel: channel.id } : null
  const path = chosen ?? await backend.pickPresetPath()
  if (path === null) return
  current(ticket)
  const review = await backend.presetRead(path)
  current(ticket)
  if (review.notes.length) toast("Preset review", { description: review.notes.join(" ") })
  if (review.preset.channel) {
    if (!channel) throw new Error("Select a channel to apply this preset")
    let withoutAudio = false
    if (review.missingAudio.length) {
      withoutAudio = await askConfirm({ title: "Preset audio is missing", description: review.missingAudio.join(", "), choices: [{ id: "settings", label: "Load settings without audio" }] }) === "settings"
      if (!withoutAudio) return
    }
    current(ticket)
    const result = await backend.presetApplyChannel(path, channel.id, channel, false, withoutAudio, state.generation, state.revision)
    if (ticket.generation === getProjectGeneration()) receivePatch(result.patch)
  } else if (review.preset.plugin) {
    if (!target) throw new Error("Select a plugin destination")
    current(ticket)
    const result = await backend.presetApplyPlugin(path, target, state.generation, state.revision)
    if (ticket.generation === getProjectGeneration()) receivePatch(result.patch)
  } else throw new Error("Use the mixer track preset panel for a mixer preset")
}
export async function saveScore() {
  const context = currentSession()?.editor.context
  const pattern = context?.pattern.id ?? currentPatternId()
  const channel = context?.channel ?? useUiStore.getState().selectedChannel
  if (pattern === null || channel === null) throw new Error("Select a piano-roll lane")
  const selection = currentSession()?.editor.selection
  const only = selection && selection.size ? [...selection].filter((id) => id >= 0) : null
  const ticket = stamp()
  const state = await backend.timelineState(); current(ticket)
  const score = await backend.scoreCapture(pattern, channel, only, state.generation, state.revision); current(ticket)
  const path = await backend.pickScorePath(score.name)
  if (path === null) return
  const written = await backend.scoreWrite(path, score)
  if (written.notes.length) toast("Score export", { description: written.notes.join(" ") })
}
export async function loadScore(mode: "replace" | "merge" = "replace") {
  const context = currentSession()?.editor.context
  const pattern = context?.pattern.id ?? currentPatternId()
  const channel = context?.channel ?? useUiStore.getState().selectedChannel
  if (pattern === null || channel === null) throw new Error("Select a piano-roll lane")
  const ticket = stamp()
  const state = await backend.timelineState(); current(ticket)
  const path = await backend.pickScorePath()
  if (path === null) return
  current(ticket)
  const review = await backend.scoreRead(path); current(ticket)
  if (review.notes.length) toast("Score review", { description: review.notes.join(" ") })
  const result = await backend.scoreApply(review.score, pattern, channel, mode, state.generation, state.revision)
  if (ticket.generation === getProjectGeneration()) receivePatch(result.patch)
}
export async function newFromTemplate(id: string) {
  if (!(await confirmDiscardChanges())) return
  await backend.templateNew(id)
}
export async function saveTemplate() {
  const name = await askText({ title: "Save as template", label: "Name", initial: useProjectStore.getState().project.settings.name, submitLabel: "Save template" })
  if (name !== null) await backend.templateSave(name)
}
const safe = (work: () => Promise<unknown>) => async () => { try { await work() } catch (error) { reportError(error, "Project file request failed") } }
export function registerProjectFilesActions() {
  return registry.register([
    { id: "files.saveChannelPreset", title: "Save channel preset…", section: "File", run: safe(() => saveSelectedPreset()) },
    { id: "files.savePluginPreset", title: "Save plugin preset…", section: "File", run: safe(() => saveSelectedPreset(true)) },
    { id: "files.loadPreset", title: "Load preset…", section: "File", run: safe(() => applyPresetFile()) },
    { id: "files.saveScore", title: "Save piano-roll score…", section: "Piano roll", run: safe(saveScore) },
    { id: "files.loadScore", title: "Replace lane from score…", section: "Piano roll", run: safe(() => loadScore()) },
    { id: "files.mergeScore", title: "Merge score into lane…", section: "Piano roll", run: safe(() => loadScore("merge")) },
    { id: "files.saveTemplate", title: "Save as template…", section: "File", run: safe(saveTemplate) },
    { id: "files.newTemplate", title: "New from template…", section: "File", run: safe(async () => {
      const templates = await backend.templates()
      const id = await askConfirm({ title: "New from template", description: "Choose a stored template.", choices: templates.map((item) => ({ id: item.id, label: item.name })) })
      if (id !== null) await newFromTemplate(id)
    }) },
  ])
}
