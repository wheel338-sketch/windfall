import type {
  BrowserEntry,
  BrowserEntryKind,
  BrowserRoot,
  SampleInfo,
  SamplePath,
} from "@/bindings"
import { message } from "@/lib/i18n"

export const FACTORY_ROOT = "/factory"

type FakeFolder = { [name: string]: FakeFolder | number }

/**
 * A made-up factory library. Leaves are sample lengths in seconds. The four
 * sounds a new project starts with are here under their real names, so its
 * samples are found.
 */
const FACTORY: FakeFolder = {
  Drums: {
    Kicks: {
      "Kick 01.wav": 0.42,
      "Kick 02.wav": 0.55,
      "Kick 03.wav": 0.3,
      "Kick Punch.wav": 0.38,
    },
    Snares: {
      "Snare 01.wav": 0.28,
      "Snare 02.wav": 0.34,
      "Snare Tight.wav": 0.22,
    },
    Claps: { "Clap 01.wav": 0.31, "Clap 02.wav": 0.4, "Clap Wide.wav": 0.36 },
    Hats: {
      "Closed Hat 01.wav": 0.09,
      "Closed Hat 02.wav": 0.12,
      "Hat Closed 1.wav": 0.07,
      "Open Hat 01.wav": 0.48,
    },
    Percussion: {
      "Rim 01.wav": 0.08,
      "Shaker 01.wav": 0.16,
      "Tom 01.wav": 0.5,
    },
  },
  Loops: { "Drum loop 128.wav": 3.75, "Top loop 140.wav": 3.43 },
}

const USER_FOLDER: FakeFolder = {
  "Recording 01.wav": 2.4,
  "Vocal chop.wav": 0.9,
  "Notes.txt": 0,
}

const AUDIO_EXTENSIONS = [".wav", ".flac", ".mp3", ".ogg"]

function kindOf(name: string): BrowserEntryKind {
  const lower = name.toLowerCase()
  if (AUDIO_EXTENSIONS.some((ext) => lower.endsWith(ext))) return "audio"
  if (lower.endsWith(".windfall")) return "project"
  return "other"
}

export function isAudioPath(path: string): boolean {
  return kindOf(path) === "audio"
}

export function baseName(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? path
  return name.replace(/\.[^.]+$/, "")
}

function rootFolder(root: BrowserRoot): FakeFolder {
  return root.kind === "factory" ? FACTORY : USER_FOLDER
}

function resolve(
  roots: BrowserRoot[],
  path: string
): FakeFolder | number | undefined {
  const root = roots.find(
    (item) => path === item.path || path.startsWith(`${item.path}/`)
  )
  if (!root) return undefined
  const parts = path.slice(root.path.length).split("/").filter(Boolean)
  let node: FakeFolder | number | undefined = rootFolder(root)
  for (const part of parts) {
    if (typeof node !== "object") return undefined
    node = node[part]
  }
  return node
}

export function defaultRoots(): BrowserRoot[] {
  return [{ name: "Factory", path: FACTORY_ROOT, kind: "factory" }]
}

/** Folders first, then by name. */
export function listFolder(roots: BrowserRoot[], path: string): BrowserEntry[] {
  const folder = resolve(roots, path)
  if (typeof folder !== "object") {
    throw new Error(message("\"{path}\" is not a folder", { path }))
  }
  return Object.entries(folder)
    .map(([name, node]): BrowserEntry => ({
      name,
      path: `${path}/${name}`,
      kind: typeof node === "object" ? "folder" : kindOf(name),
    }))
    .sort((a, b) => {
      const folders = Number(b.kind === "folder") - Number(a.kind === "folder")
      return folders || a.name.localeCompare(b.name)
    })
}

export function samplePathFor(path: string): SamplePath {
  if (path.startsWith(`${FACTORY_ROOT}/`)) {
    return { kind: "factory", path: path.slice(FACTORY_ROOT.length + 1) }
  }
  return { kind: "external", path }
}

function hash(text: string): number {
  let value = 2166136261
  for (let index = 0; index < text.length; index += 1) {
    value ^= text.charCodeAt(index)
    value = Math.imul(value, 16777619)
  }
  return value >>> 0
}

/** Small seeded generator, so a file always draws the same waveform. */
function randomFrom(seed: number): () => number {
  let state = seed || 1
  return () => {
    state ^= state << 13
    state ^= state >>> 17
    state ^= state << 5
    return ((state >>> 0) % 10_000) / 10_000
  }
}

const PEAK_BUCKETS = 256

export function sampleInfoFor(roots: BrowserRoot[], path: string): SampleInfo {
  const node = resolve(roots, path)
  if (typeof node !== "number" || !isAudioPath(path)) {
    throw new Error(message("\"{path}\" is not an audio file Windfall can read", { path }))
  }
  const random = randomFrom(hash(path))
  const sampleRate = 44_100
  const frames = Math.round(node * sampleRate)
  const sustained = node > 1.5
  const peaks: number[] = []
  for (let bucket = 0; bucket < PEAK_BUCKETS; bucket += 1) {
    const position = bucket / PEAK_BUCKETS
    const envelope = sustained
      ? 0.55 + 0.4 * Math.abs(Math.sin(position * Math.PI * 8))
      : Math.exp(-position * 5)
    const height = envelope * (0.6 + 0.4 * random())
    peaks.push(-height * (0.8 + 0.2 * random()), height)
  }
  return {
    path,
    name: baseName(path),
    sampleRate,
    channels: sustained ? 2 : 1,
    frames,
    durationSecs: node,
    peaks,
  }
}
