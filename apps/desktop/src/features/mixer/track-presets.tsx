import { useEffect, useRef, useState } from "react"
import { create } from "zustand"
import type {
  MixerTrack,
  MixerTrackPreset,
  PluginEntry,
  TrackId,
} from "@/bindings"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { backend, errorMessage } from "@/lib/ipc"
import type {
  MixerPresetLibrary,
  MixerPresetLibraryEntry,
} from "@/lib/ipc/mixer-presets"
import { dispatch, useProjectStore } from "@/lib/store"
import { getProjectGeneration } from "@/lib/store/replaced"

type SavedPreset = { key: string; preset: MixerTrackPreset }
const STORAGE_KEY = "windfall.mixer-presets.v1"
function readShelf(): SavedPreset[] {
  try {
    const data: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]")
    return Array.isArray(data) ? data.filter((item) => item && typeof item.key === "string" && item.preset?.version === 1 && Array.isArray(item.preset.effects)).slice(0, 128) : []
  } catch { return [] }
}
const useShelf = create<{ entries: SavedPreset[] }>(() => ({ entries: readShelf() }))
function writeShelf(entries: SavedPreset[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(entries))
  useShelf.setState({ entries })
}

/** What a preset needs to know about a hosted plugin to look for it. */
type PluginRef = { format: string; id: string; name: string }

/**
 * The names of the plugins a preset refers to that are not installed and
 * usable here, each once, in the preset's order. A plugin is the same one
 * when its format and id match: its file may be anywhere on this machine.
 */
export function missingPlugins(
  wanted: readonly PluginRef[],
  installed: readonly PluginEntry[]
): string[] {
  const have = new Set(
    installed
      .filter((entry) => entry.usable)
      .map((entry) => `${entry.format.toLowerCase()}\n${entry.id}`)
  )
  const missing: string[] = []
  for (const plugin of wanted) {
    if (have.has(`${plugin.format.toLowerCase()}\n${plugin.id}`)) continue
    const name = plugin.name || plugin.id
    if (!missing.includes(name)) missing.push(name)
  }
  return missing
}

const KEPT =
  "Their settings are kept with the project, and those effects pass audio through until the plugins are installed."

function missingNote(missing: readonly string[]): string {
  return `Not installed here: ${missing.join(", ")}`
}

export function TrackPresetsPanel({ track }: { track: TrackId }) {
  const trackName = useProjectStore((state) => state.project.mixer.tracks.find((item) => item.id === track)?.name ?? "Mixer track")
  const entries = useShelf((state) => state.entries)
  const [name, setName] = useState(trackName)
  const [nameColor, setNameColor] = useState(false)
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState("")
  const [library, setLibrary] = useState<MixerPresetLibrary | null>(null)
  const [libraryError, setLibraryError] = useState("")
  // `null` until the plugin manager has answered, and when it cannot.
  const [installed, setInstalled] = useState<PluginEntry[] | null>(null)
  const [confirming, setConfirming] = useState<string | null>(null)
  const alive = useRef(true)
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
    }
  }, [])

  /** Reads the library folder and what is installed. Never throws. */
  async function refresh() {
    try {
      const [listed, plugins] = await Promise.all([
        backend.mixerPresetLibrary(),
        backend.pluginsState().then(
          (state) => state.entries,
          () => null
        ),
      ])
      if (!alive.current) return
      setLibrary(listed)
      setInstalled(plugins)
      setLibraryError("")
    } catch (error) {
      if (alive.current) setLibraryError(errorMessage(error))
    }
  }

  async function capture() {
    const generation = getProjectGeneration()
    const guard = await backend.timelineState()
    const preset = await backend.mixerPresetCapture(track, guard.generation, guard.revision)
    if (getProjectGeneration() !== generation) throw new Error("The project changed during preset capture")
    return { ...preset, name: name.trim() || trackName }
  }
  async function run(work: () => Promise<void>) {
    if (busy) return
    setBusy(true); setMessage("")
    try { await work() } catch (error) { if (alive.current) setMessage(errorMessage(error)) } finally { if (alive.current) setBusy(false) }
  }
  function destination(): MixerTrack {
    const expected = structuredClone(useProjectStore.getState().project.mixer.tracks.find((item) => item.id === track))
    if (!expected) throw new Error("The destination track no longer exists")
    return expected
  }
  async function apply(preset: MixerTrackPreset, generation: number, expected: MixerTrack) {
    // Asked again at the moment of loading: a scan may have finished since
    // the list was drawn.
    const plugins = preset.plugins.length
      ? await backend.pluginsState().then(
          (state) => state.entries,
          () => null
        )
      : []
    if (getProjectGeneration() !== generation) throw new Error("The project changed before loading the preset")
    if (!(await dispatch({ type: "applyMixerTrackPreset", id: track, expected, preset, nameColor }))) {
      setMessage("Preset was not applied. The destination may have changed.")
      return
    }
    if (plugins === null) {
      setMessage(`Loaded ${preset.name}. Could not check which of its plugins are installed.`)
      return
    }
    const missing = missingPlugins(preset.plugins, plugins)
    setMessage(missing.length ? `Loaded ${preset.name}. ${missingNote(missing)}. ${KEPT}` : `Loaded ${preset.name}`)
  }
  async function applyFromLibrary(entry: MixerPresetLibraryEntry) {
    const generation = getProjectGeneration()
    const expected = destination()
    const preset = await backend.mixerPresetLibraryRead(entry.file)
    await apply(preset, generation, expected)
  }

  return <div className="shrink-0 border-b p-1.5"><Popover onOpenChange={(open) => { if (open) void refresh(); else setConfirming(null) }}>
    <PopoverTrigger render={<Button size="sm" variant="outline" className="w-full justify-start">Track presets</Button>} />
    <PopoverContent align="start" className="w-80 space-y-3">
      <Input aria-label="Mixer preset name" value={name} onChange={(event) => setName(event.target.value)} disabled={busy} maxLength={120} />
      <div className="flex flex-wrap gap-1">
        <Button size="sm" variant="outline" disabled={busy} onClick={() => void run(async () => {
          const saved = await backend.mixerPresetLibrarySave(await capture())
          setMessage(`Saved ${saved.name} to the preset library as ${saved.file}`)
          await refresh()
        })}>Save to library</Button>
        <Button size="sm" variant="outline" disabled={busy || entries.length >= 128} onClick={() => void run(async () => {
          const preset = await capture()
          writeShelf([...useShelf.getState().entries, { key: crypto.randomUUID(), preset }])
          setMessage(`Saved ${preset.name} to the preset shelf`)
        })}>Save to shelf</Button>
        <Button size="sm" variant="outline" disabled={busy} onClick={() => void run(async () => {
          const path = await backend.mixerPresetSave(await capture())
          if (path) setMessage(`Saved ${path}`)
        })}>Save file…</Button>
        <Button size="sm" variant="outline" disabled={busy} onClick={() => void run(async () => {
          const generation = getProjectGeneration()
          const expected = destination()
          const preset = await backend.mixerPresetLoad()
          if (preset) await apply(preset, generation, expected)
        })}>Load file…</Button>
      </div>
      <label className="flex items-center gap-2 text-xs"><Checkbox checked={nameColor} onCheckedChange={(checked) => setNameColor(checked === true)} disabled={busy} />Restore preset name and color</label>
      <p className="text-xs text-muted-foreground">Restores effects, their saved plugin state, fader, pan, mute, EQ, stereo controls and latency correction. Existing effect automation is removed. Routing, recording, dock and hardware assignments stay with this track.</p>
      <section aria-label="Mixer preset library" className="space-y-1">
        <p className="text-xs font-medium">Preset library</p>
        {libraryError && <p role="alert" className="break-words text-xs text-destructive">{libraryError}</p>}
        {library && library.entries.length === 0 && <p className="break-words text-xs text-muted-foreground">No presets in {library.folder} yet. Save to library puts one there.</p>}
        {library && library.entries.length > 0 && <ul aria-label="Library presets" className="max-h-48 space-y-1 overflow-y-auto">
          {library.entries.map((entry) => {
            const missing = installed ? missingPlugins(entry.plugins, installed) : []
            return <li key={entry.file} className="space-y-0.5">
              <div className="flex items-center gap-1">
                <Button size="sm" variant="ghost" className="min-w-0 flex-1 justify-start truncate" title={entry.path} disabled={busy || entry.error !== null} onClick={() => void run(() => applyFromLibrary(entry))}>{entry.name} · {entry.effects} FX</Button>
                {confirming === entry.file
                  ? <Button size="sm" variant="destructive" aria-label={`Confirm deleting library preset ${entry.name}`} disabled={busy} onClick={() => void run(async () => {
                      setConfirming(null)
                      await backend.mixerPresetLibraryDelete(entry.file)
                      setMessage(`Deleted ${entry.file} from the preset library`)
                      await refresh()
                    })}>Delete file</Button>
                  : <Button size="sm" variant="ghost" aria-label={`Delete library preset ${entry.name}`} disabled={busy} onClick={() => setConfirming(entry.file)}>×</Button>}
              </div>
              {entry.error !== null && <p className="break-words pl-2 text-[10px] text-destructive">{entry.file}: {entry.error}</p>}
              {missing.length > 0 && <p className="break-words pl-2 text-[10px] text-muted-foreground">{missingNote(missing)}</p>}
            </li>
          })}
        </ul>}
        {library?.truncated && <p className="text-[10px] text-muted-foreground">The library folder holds more presets than are listed here.</p>}
      </section>
      <p className="text-xs font-medium">Shelf</p>
      <ul aria-label="Saved mixer presets" className="max-h-48 space-y-1 overflow-y-auto">
        {entries.map((entry) => {
          const missing = installed ? missingPlugins(entry.preset.plugins ?? [], installed) : []
          return <li key={entry.key} className="space-y-0.5">
            <div className="flex items-center gap-1">
              <Button size="sm" variant="ghost" className="min-w-0 flex-1 justify-start truncate" disabled={busy} onClick={() => void run(async () => {
                await apply(entry.preset, getProjectGeneration(), destination())
              })}>{entry.preset.name} · {entry.preset.effects.length} FX</Button>
              <Button size="sm" variant="ghost" aria-label={`Delete preset ${entry.preset.name}`} disabled={busy} onClick={() => void run(async () => writeShelf(useShelf.getState().entries.filter((item) => item.key !== entry.key)))}>×</Button>
            </div>
            {missing.length > 0 && <p className="break-words pl-2 text-[10px] text-muted-foreground">{missingNote(missing)}</p>}
          </li>
        })}
      </ul>
      {message && <p role="status" className="break-words text-xs">{message}</p>}
    </PopoverContent>
  </Popover></div>
}
