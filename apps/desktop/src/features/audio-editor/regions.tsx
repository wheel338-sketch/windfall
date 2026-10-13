import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { Choice } from "./tools-panel"
import type { AudioRegion, AudioEditPreview } from "./types"
import type { Selection } from "./selection"

const byteLength = (text: string) => new TextEncoder().encode(text).length

function RegionRow({ region, index, preview, disabled, onSelect, onSave, onDelete }: {
  region: AudioRegion; index: number; preview: AudioEditPreview; disabled: boolean
  onSelect(selection: Selection): void; onSave(region: AudioRegion): void; onDelete(): void
}) {
  const [name, setName] = useState(region.name)
  const [entries, setEntries] = useState(region.notes ?? [])
  const notes = entries.join("\n")
  const [start, setStart] = useState(region.startFrame)
  const [end, setEnd] = useState(region.endFrame)
  // Names and typed attribute edits preserve each imported note, including
  // embedded newlines. Only explicit edits of the raw note list split lines.
  function attribute(prefix: string, value: string) {
    const fields = value.split(" ")
    const updated = entries.filter(note => !note.startsWith(prefix)).map(note => {
      // The exact beat note and ACID header describe the same source clock.
      // Keep their editable controls consistent before the checked commit.
      if (prefix === "WindfallBeats/1 " && note.startsWith("WindfallTempo/1 ")) {
        const tempoFields = note.split(" ").slice(1)
        tempoFields[0] = fields[0] ?? ""
        const beats = Number(fields[1])
        tempoFields[1] = Number.isInteger(beats) ? String(beats) : "0"
        return `WindfallTempo/1 ${tempoFields.join(" ")}`
      }
      if (prefix === "WindfallTempo/1 " && note.startsWith("WindfallBeats/1 ")) {
        return `WindfallBeats/1 ${fields[0] ?? ""} ${note.split(" ")[2] ?? "0"}`
      }
      return note
    })
    setEntries([...updated, `${prefix}${value}`])
  }
  const slice = entries.find(note => note.startsWith("WindfallSlice/1 "))?.split(" ").slice(1)
  const beats = entries.find(note => note.startsWith("WindfallBeats/1 "))?.split(" ").slice(1)
  const tempo = entries.find(note => note.startsWith("WindfallTempo/1 "))?.split(" ").slice(1)
  const root = entries.find(note => note.startsWith("WindfallRoot/1 "))?.split(" ")[1]
  const loop = entries.find(note => note.startsWith("WindfallLoop/1 "))?.split(" ").slice(1)
  const navigation = entries.find(note => note.startsWith("WindfallNavigation/1 "))?.split(" ")[1] ?? "named"
  const valid = name.trim().length > 0 && byteLength(name.trim()) <= 256 &&
    entries.length <= 16 && entries.every(note => byteLength(note) <= 1024 && !note.includes("\0")) &&
    [start, end].every(frame => Number.isSafeInteger(frame) && frame >= 0 && frame <= preview.frames) && start <= end
  function save() {
    const updated = entries.map(note => /^WindfallNavigation\/1 (loop|skip) /.test(note)
      ? `${note.split(" ").slice(0, 2).join(" ")} ${end}` : note)
    onSave({ name: name.trim(), startFrame: start, endFrame: end, notes: updated })
  }
  return <li className="space-y-2 rounded-md border p-2">
    <Button size="sm" variant="ghost" disabled={disabled} onClick={() => onSelect({
      start: Math.min(region.startFrame, preview.frames - 1),
      end: region.endFrame === region.startFrame ? Math.min(region.endFrame + 1, preview.frames) : region.endFrame,
    })}>{region.name} · {region.startFrame}–{region.endFrame}</Button>
    <div className="flex flex-wrap gap-2">
      <Input aria-label={`Name of annotation ${index + 1}`} value={name} disabled={disabled} onChange={event => setName(event.target.value)} />
      <Input type="number" aria-label={`Annotation ${index + 1} start frame`} value={start} disabled={disabled} onChange={event => setStart(event.target.valueAsNumber)} />
      <Input type="number" aria-label={`Annotation ${index + 1} end frame`} value={end} disabled={disabled} onChange={event => setEnd(event.target.valueAsNumber)} />
      <Choice label={`Annotation ${index + 1} navigation`} value={navigation} disabled={disabled}
        options={[{ value: "named", label: "Named" }, { value: "loop", label: "Loop" }, { value: "skip", label: "Skip" }, { value: "pause", label: "Pause" }]}
        onChange={value => setEntries([...entries.filter(note => !note.startsWith("WindfallNavigation/1 ") && !note.startsWith("WindfallLoop/1 ")),
          ...(value === "named" ? [] : [`WindfallNavigation/1 ${value}${value === "pause" ? "" : ` ${end}`}`])])} />
    </div>
    {slice && <fieldset className="flex flex-wrap gap-2"><legend className="text-sm">Slice attributes</legend>
      {["Gain", "Pitch semitones", "Attack ms", "Release ms", "Reverse (0 or 1)", "Deck (0 or 1)"].map((label, at) =>
        <label key={label} className="text-sm">{label}<Input type="number" step="any" aria-label={`${region.name} ${label}`} value={slice[at] ?? ""} disabled={disabled}
          onChange={event => attribute("WindfallSlice/1 ", slice.map((value, i) => i === at ? event.target.value : value).join(" "))} /></label>)}
    </fieldset>}
    {beats && <fieldset className="flex gap-2"><legend className="text-sm">Source beats</legend>
      {["BPM", "Beats"].map((label, at) => <label key={label} className="text-sm">{label}<Input type="number" step="any" aria-label={`${region.name} ${label}`} value={beats[at] ?? ""} disabled={disabled}
        onChange={event => attribute("WindfallBeats/1 ", beats.map((value, i) => i === at ? event.target.value : value).join(" "))} /></label>)}
    </fieldset>}
    {tempo && <fieldset className="flex flex-wrap gap-2"><legend className="text-sm">WAV tempo and meter</legend>
      {["Tempo BPM", "Integer beats", "Meter numerator", "Meter denominator", "Tempo root key (-1 for none)", "One shot (0 or 1)"].map((label, at) =>
        <label key={label} className="text-sm">{label}<Input type="number" step="any" aria-label={`${region.name} ${label}`} value={tempo[at] ?? ""} disabled={disabled}
          onChange={event => attribute("WindfallTempo/1 ", tempo.map((value, i) => i === at ? event.target.value : value).join(" "))} /></label>)}
    </fieldset>}
    {root !== undefined && <label className="text-sm">Root MIDI key<Input type="number" min={0} max={127} aria-label={`${region.name} Root MIDI key`} value={root} disabled={disabled}
      onChange={event => attribute("WindfallRoot/1 ", event.target.value)} /></label>}
    {loop && <fieldset className="flex gap-2"><legend className="text-sm">Sample loop</legend>
      <Choice label={`${region.name} Loop direction`} value={loop[0] ?? "forward"} disabled={disabled}
        options={[{value: "forward", label: "Forward"}, {value: "alternating", label: "Alternating"}, {value: "backward", label: "Backward"}]}
        onChange={value => attribute("WindfallLoop/1 ", `${value} ${loop[1] ?? "0"}`)} />
      <label className="text-sm">Repeat count (0 is infinite)<Input type="number" min={0} step={1} aria-label={`${region.name} Loop repeat count`} value={loop[1] ?? "0"} disabled={disabled}
        onChange={event => attribute("WindfallLoop/1 ", `${loop[0] ?? "forward"} ${event.target.value}`)} /></label>
      {(loop[0] !== "forward" || loop[1] !== "0") && <p className="text-xs text-muted-foreground">This sample loop imports to the song timeline as named boundaries; the song loop cannot play its direction or finite repeat count.</p>}
    </fieldset>}
    <label className="block text-sm">Cue notes
      <Textarea aria-label={`Notes of annotation ${index + 1}`} value={notes} disabled={disabled} onChange={event => setEntries(event.target.value.split("\n").filter(Boolean))} />
    </label>
    <p className="text-xs text-muted-foreground">One cue note per line. Source attributes and navigation notes travel with the WAV.</p>
    <div className="flex flex-wrap gap-2">
      <Button size="sm" variant="outline" disabled={disabled || !valid} onClick={save}>Save annotation {index + 1}</Button>
      <Button size="sm" variant="ghost" disabled={disabled} aria-label={`Remove ${region.name}`} onClick={onDelete}>Remove</Button>
      {!slice && start < end && <Button size="sm" variant="outline" disabled={disabled} onClick={() => attribute("WindfallSlice/1 ", "1 0 1 4 0 0")}>Add slice attributes</Button>}
      {!beats && <Button size="sm" variant="outline" disabled={disabled} onClick={() => attribute("WindfallBeats/1 ", `${tempo?.[0] ?? "120"} ${tempo?.[1] && tempo[1] !== "0" ? tempo[1] : "4"}`)}>Add beat metadata</Button>}
    </div>
  </li>
}

export function Regions({preview,selection,disabled,onSelect,onWrite}: {
  preview: AudioEditPreview; selection: Selection; disabled: boolean
  onSelect(selection: Selection): void; onWrite(regions: AudioRegion[]): void
}) {
  const [name,setName] = useState("Region")
  const regions=preview.regions ?? []
  const canAdd=!disabled && name.trim().length>0 && byteLength(name.trim())<=256 && regions.length<256
  function add(marker: boolean) { onWrite([...regions,{name:name.trim(),startFrame:selection.start,endFrame:marker ? selection.start : selection.end}]) }
  return <section aria-label="Audio regions and markers" className="rounded-md border p-2">
    <label htmlFor="editor-region-name" className="text-sm">Region or marker name</label>
    <Input id="editor-region-name" value={name} maxLength={256} disabled={disabled} onChange={event=>setName(event.target.value)} />
    <div className="flex gap-2 py-2"><Button size="sm" variant="outline" disabled={!canAdd} onClick={()=>add(false)}>Add region</Button><Button size="sm" variant="outline" disabled={!canAdd} onClick={()=>add(true)}>Add marker at selection start</Button></div>
    <ul className="space-y-2">{regions.map((region,i)=><RegionRow key={`${preview.token}-${i}`} region={region} index={i} preview={preview} disabled={disabled}
      onSelect={onSelect} onDelete={()=>onWrite(regions.filter((_,index)=>index!==i))}
      onSave={updated=>onWrite(regions.map((value,index)=>index===i ? updated : value))} />)}</ul>
  </section>
}
