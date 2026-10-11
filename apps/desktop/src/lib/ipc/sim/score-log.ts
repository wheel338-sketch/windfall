import type { ChannelId, Command, NoteInit, Project, ScoreLogState } from "@/bindings"
import { PPQ } from "@/lib/units"

const SPAN = 30 * 60 * 1000
const LIMIT = 16_384
const KEY = "windfall.score-log.v2"
type Note = { channel: ChannelId; key: number; velocity: number; on: number; off: number }

/** Browser audition log: actual accepted note calls, with bounded restart storage. */
export function createBrowserScoreLog(storage: Pick<Storage, "getItem" | "setItem"> | null) {
  let notes: Note[] = []
  const open = new Map<string, Omit<Note, "off">>()
  let identity: string | null = null
  let error: string | null = null
  try {
    const saved: unknown = JSON.parse(storage?.getItem(KEY) ?? "null")
    if (saved && typeof saved === "object" && "notes" in saved && Array.isArray(saved.notes)) {
      notes = saved.notes.filter((note): note is Note => note && typeof note === "object" &&
        Number.isSafeInteger(note.channel) && Number.isInteger(note.key) && note.key >= 0 && note.key <= 127 &&
        Number.isFinite(note.velocity) && note.velocity >= 0 && note.velocity <= 1 &&
        Number.isFinite(note.on) && Number.isFinite(note.off) && note.off >= note.on && note.on <= Date.now())
        .slice(-LIMIT)
      if ("identity" in saved && typeof saved.identity === "string") identity = saved.identity
    }
  } catch { error = "The saved note log could not be read." }
  function prune() { notes = notes.filter((note) => Date.now() - note.on <= SPAN) }
  function persist() {
    prune()
    try { storage?.setItem(KEY, JSON.stringify({ identity, notes })); error = null }
    catch { error = "The note log could not be saved in browser storage." }
  }
  function sync(project: Project) {
    const next = JSON.stringify([project.settings.name, project.channels.map((channel) => [channel.id, channel.name, channel.source.type])])
    if (identity !== null && identity !== next) { notes = []; open.clear() }
    identity = next
    prune()
  }
  return {
    note(project: Project, channel: ChannelId, key: number, velocity: number | null) {
      sync(project)
      if (!project.channels.some((item) => item.id === channel)) return
      const id = `${channel}:${key}`
      const held = open.get(id)
      if (held) { notes.push({ ...held, off: Date.now() }); open.delete(id) }
      if (velocity !== null) {
        if (open.size >= 256) open.delete(open.keys().next().value!)
        open.set(id, { channel, key, velocity: Math.min(1, Math.max(0, velocity)), on: Date.now() })
      }
      notes = notes.slice(-LIMIT)
      persist()
    },
    state(project: Project): ScoreLogState { sync(project); return { notes: notes.length, seconds: notes[0] ? (Date.now() - notes[0].on) / 1000 : 0 } },
    error: () => error,
    clear() { notes = []; open.clear(); persist() },
    command(project: Project, pattern: number, seconds: number | null, quantize: number): Command {
      sync(project)
      if (!project.patterns.some((item) => item.id === pattern)) throw new Error("The pattern no longer exists.")
      const picked = notes.filter((note) => seconds === null || Date.now() - note.on <= seconds * 1000).sort((a, b) => a.on - b.on)
      if (picked.length === 0) throw new Error("Nothing has been played to put into the pattern.")
      const rate = project.settings.tempoBpm * PPQ / 60 / 1000
      const lanes = new Map<ChannelId, NoteInit[]>()
      for (const note of picked) {
        if (!project.channels.some((channel) => channel.id === note.channel)) continue
        const raw = (note.on - picked[0]!.on) * rate
        const start = Math.round(quantize ? Math.round(raw / quantize) * quantize : raw)
        if (start > 245760 - 1) continue
        const lane = lanes.get(note.channel) ?? []
        lane.push({ start, length: Math.max(1, Math.min(245760, Math.round((note.off - note.on) * rate))), key: note.key, velocity: note.velocity })
        lanes.set(note.channel, lane)
      }
      if (!lanes.size) throw new Error("No logged channels remain in the project.")
      return { type: "batch", label: "Dump note log", commands: [...lanes].map(([channel, notes]) => ({ type: "addNotes", pattern, channel, notes })) }
    },
  }
}
