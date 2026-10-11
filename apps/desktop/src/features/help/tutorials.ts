import type { Project } from "@/bindings"
import { runAction } from "@/lib/actions"
import { currentPatternId } from "@/lib/flows/edit"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { setPlayMode } from "@/lib/store/transport"
export type TutorialKind = "beat" | "record" | "import" | "export"
export type Baseline = { channels: number[]; notes: number; clips: number[]; samples: number[] }
export function baseline(project: Project): Baseline {
  return { channels: project.channels.map((channel) => channel.id), notes: project.patterns.reduce((sum, pattern) => sum + pattern.lanes.reduce((sum, lane) => sum + lane.notes.length, 0), 0), clips: project.playlist.clips.map((clip) => clip.id), samples: project.samples.map((sample) => sample.id) }
}
export type TutorialEvidence = { project: Project; before: Baseline; playing: boolean; song: boolean; recording: boolean; inputs: number; recordingOpen: boolean; exportOpen: boolean; exported: boolean; dirty: boolean }
export type TutorialStep = { title: string; instruction: string; action: string; complete(evidence: TutorialEvidence): boolean }
const notesAdded = (e: TutorialEvidence) => baseline(e.project).notes > e.before.notes
const audioAdded = (e: TutorialEvidence) => e.project.playlist.clips.some((clip) => !e.before.clips.includes(clip.id) && clip.content.type === "audio")
export const TUTORIALS: Record<TutorialKind, { title: string; steps: TutorialStep[] }> = {
  beat: { title: "Build a beat", steps: [
    { title: "Add a channel", instruction: "Add a sound channel or use the factory drum kit already in the rack.", action: "channel.add", complete: (e) => e.project.channels.length > 0 },
    { title: "Open the piano roll", instruction: "Draw notes or turn on drum steps in the rack. Keep the guide open while editing.", action: "view.pianoRoll", complete: notesAdded },
    { title: "Open the playlist", instruction: "Place the current pattern on a playlist lane using the button, or draw a pattern clip yourself.", action: "tutorial.placePattern", complete: (e) => e.project.playlist.clips.some((clip) => !e.before.clips.includes(clip.id) && clip.content.type === "pattern") },
    { title: "Play", instruction: "Play the song, then stop and save your beat.", action: "tutorial.playSong", complete: (e) => e.playing && e.song },
    { title: "Save", instruction: "Save the project. This step completes only when the document is clean.", action: "file.save", complete: (e) => !e.dirty },
  ] },
  record: { title: "Record audio", steps: [
    { title: "Record audio input", instruction: "Open recording and choose a real input device. Browser mode reports its native limitation.", action: "recording.open", complete: (e) => e.recordingOpen && e.inputs > 0 },
    { title: "Start recording", instruction: "Select the input channels, timing and destination, then start recording.", action: "recording.start", complete: (e) => e.recording },
    { title: "Stop recording and keep take", instruction: "Stop and keep the take. Completion requires a new audio clip in the playlist.", action: "recording.stop", complete: audioAdded },
  ] },
  import: { title: "Import audio", steps: [
    { title: "Import audio", instruction: "Choose an audio file. A cancelled or failed picker does not complete the step.", action: "channel.addFromFile", complete: (e) => e.project.samples.some((sample) => !e.before.samples.includes(sample.id)) && e.project.channels.some((channel) => !e.before.channels.includes(channel.id)) },
    { title: "Open the piano roll", instruction: "Draw a note for the imported sampler, or turn on a step in the rack.", action: "view.pianoRoll", complete: notesAdded },
    { title: "Save", instruction: "Save the project and its sample reference before closing it.", action: "file.save", complete: (e) => !e.dirty },
  ] },
  export: { title: "Export audio", steps: [
    { title: "Export", instruction: "Open Export audio and choose pattern or song, file format and destination.", action: "file.export", complete: (e) => e.exportOpen },
    { title: "Export audio", instruction: "Press Export in the real export dialog. Only successful completion counts; cancellation and errors do not.", action: "file.export", complete: (e) => e.exported },
  ] },
}

/** Ordinary document/transport commands; never injects tutorial-only app state. */
export async function runTutorialAction(action: string): Promise<void> {
  if (action === "tutorial.playSong") { await setPlayMode("song"); await runAction("transport.play"); return }
  if (action !== "tutorial.placePattern") { await runAction(action); return }
  const generation = getProjectGeneration()
  const pattern = currentPatternId()
  if (pattern === null) return
  let track = useProjectStore.getState().project.playlist.tracks[0]?.id
  if (track === undefined) {
    const added = await dispatch({ type: "addPlaylistTrack" })
    if (!added || generation !== getProjectGeneration()) return
    track = added.created[0]
  }
  if (generation !== getProjectGeneration()) return
  const result = await dispatch({ type: "addClips", clips: [{ track, start: 0, content: { type: "pattern", pattern } }] })
  if (result) await runAction("view.playlist")
}
