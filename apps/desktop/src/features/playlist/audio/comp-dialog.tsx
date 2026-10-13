import { create } from "zustand"
import { useState } from "react"
import type { AudioCompSegment, AudioTakeGroup, Clip } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { errorMessage } from "@/lib/ipc"
import { onProjectReplaced } from "@/lib/store/replaced"
import { PPQ } from "@/lib/units"
import { usePlaylistStore } from "../store"
import { selectedAudioClips } from "./ops"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const useComp = create(() => ({ sources: null as Clip[] | null, serial: 0, group: null as AudioTakeGroup | null }))
onProjectReplaced(() => useComp.setState({ sources: null, group: null }))
export function openAudioComp() {
  const clips = selectedAudioClips()
  if (clips.length) useComp.setState((state) => ({ sources: structuredClone(clips), group: null, serial: state.serial + 1 }))
}
export function openTakeGroupComp(id?: number) {
  const playlist = useProjectStore.getState().project.playlist
  const selected = usePlaylistStore.getState().selection
  const group = playlist.takeGroups?.find((group) => id === undefined ? group.lanes.some((lane) => lane.takes.some((take) => selected.has(take.clip))) || group.comp?.some((clip) => selected.has(clip)) : group.id === id)
  if (!group?.lanes.length) return
  const ids = new Set(group.lanes.flatMap((lane) => lane.takes.map((take) => take.clip)))
  const sources = playlist.clips.filter((clip) => ids.has(clip.id))
  useComp.setState((state) => ({ sources: structuredClone(sources), group: structuredClone(group), serial: state.serial + 1 }))
}
export function AudioCompDialog() {
  const sources = useComp((state) => state.sources)
  const serial = useComp((state) => state.serial)
  const group = useComp((state) => state.group)
  return sources ? <CompEditor key={serial} sources={sources} group={group} /> : null
}
function ranges(sources: Clip[], size: number): AudioCompSegment[] {
  const start = Math.max(...sources.map((clip) => clip.start))
  const end = Math.min(...sources.map((clip) => clip.start + clip.length))
  const first = start < end ? start : sources[0].start
  const last = start < end ? end : sources[0].start + sources[0].length
  const step = Math.max(Math.round(size * PPQ), Math.ceil((last - first) / 128), 1)
  const result: AudioCompSegment[] = []
  for (let at = first; at < last; at += step) {
    const stop = Math.min(last, at + step)
    const source = sources.findLast((clip) => clip.start <= at && clip.start + clip.length >= stop) ?? sources[0]
    result.push({ clip: source.id, start: at, end: stop })
  }
  return result
}
function CompEditor({ sources, group }: { sources: Clip[], group: AudioTakeGroup | null }) {
  const samples = useProjectStore((state) => state.project.samples)
  const commonPasses = group?.lanes[0]?.takes.filter((take) => group.lanes.every((lane) => lane.takes.some((item) => item.pass === take.pass))) ?? []
  const options = group ? sources.filter((source) => commonPasses.some((take) => take.clip === source.id)) : sources
  const [segments, setSegments] = useState(() => options.length ? ranges(options, 1) : [])
  const [size, setSize] = useState(1)
  const [name, setName] = useState("Comped takes")
  const [fade, setFade] = useState(0.05)
  const [mute, setMute] = useState(true)
  const [replace, setReplace] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const patch = (index: number, change: Partial<AudioCompSegment>) => setSegments((rows) => rows.map((row, i) => i === index ? { ...row, ...change } : row))
  const close = () => { if (!busy) useComp.setState({ sources: null }) }
  const createComp = async () => {
    if (busy) return
    setBusy(true); setError("")
    try {
      const result = await dispatch(group
        ? { type: "compAudioTakeGroup", expected: group, sources, ranges: segments.map((segment) => ({ pass: commonPasses.find((take) => take.clip === segment.clip)?.pass ?? 256, start: segment.start, end: segment.end })), name, fadeTicks: Math.max(0, Math.round(fade * PPQ)), muteSources: mute, replaceComp: replace }
        : { type: "compAudioClips", sources, segments, destination: null, name, fadeTicks: Math.max(0, Math.round(fade * PPQ)), muteSources: mute })
      if (!result) { setError(t("The comp was not applied. Resolve the project error or reopen the source selection.")); return }
      if (useComp.getState().sources !== sources) return
      const live = new Set(useProjectStore.getState().project.playlist.clips.map((clip) => clip.id))
      usePlaylistStore.getState().select(result.created.filter((id) => live.has(id)))
      useComp.setState({ sources: null })
    } catch (error) { setError(errorMessage(error)) }
    finally { setBusy(false) }
  }
  return <Dialog open onOpenChange={(open) => { if (!open) close() }}>
    <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-3xl">
      <DialogHeader><DialogTitle>{group ? message("Comp {groupName}", { groupName: group.name }) : t("Comp audio takes")}</DialogTitle><DialogDescription>{group ? message("Each range chooses one recorded pass for all {count} input lanes. Crossfades use the same boundaries on every lane.", { count: plainNumber(group.lanes.length) }) : t("Choose a source for each song range. The composite uses editable clips on a new lane and retains every original take.")}</DialogDescription></DialogHeader>
      <div className="grid grid-cols-2 gap-3">
        <Field><FieldLabel htmlFor="comp-name">{t("Composite lane")}</FieldLabel><Input id="comp-name" value={name} disabled={busy} onChange={(event) => setName(event.target.value)} /></Field>
        <Field><FieldLabel htmlFor="comp-fade">{t("Crossfade (beats)")}</FieldLabel><Input id="comp-fade" type="number" min={0} max={4} step={0.01} value={fade} disabled={busy} onChange={(event) => { const value = Number(event.target.value); if (Number.isFinite(value)) setFade(Math.max(0, Math.min(4, value))) }} /></Field>
      </div>
      <Field><FieldLabel htmlFor="comp-size">{t("Range size (beats)")}</FieldLabel><div className="flex gap-2"><Input id="comp-size" type="number" min={0.25} max={64} step={0.25} value={size} disabled={busy} onChange={(event) => { const value = Number(event.target.value); if (Number.isFinite(value)) setSize(Math.max(0.25, Math.min(64, value))) }} /><Button variant="outline" disabled={busy || !options.length} onClick={() => setSegments(ranges(options, size))}>{t("Reset ranges")}</Button></div><FieldDescription>{t("Positions are absolute song beats, starting at zero. Adjacent ranges from different takes receive bounded equal-power overlaps.")}</FieldDescription></Field>
      <div className="max-h-80 overflow-y-auto rounded-md border">
        <table className="w-full text-sm"><thead className="bg-muted text-left"><tr><th className="p-2">{t("From beat")}</th><th className="p-2">{t("To beat")}</th><th className="p-2">{t("Source take")}</th><th className="p-2">{t("Edit")}</th></tr></thead><tbody>{segments.map((segment, index) => {
          const items = options.map((clip, ordinal) => {
            const sampleId = clip.content.type === "audio" ? clip.content.sample : null
            return { value: clip.id, label: group ? message("Pass {pass}", { pass: plainNumber((commonPasses.find((take) => take.clip === clip.id)?.pass ?? 0) + 1) }) : message("{index}. {title}", { index: ordinal + 1, title: samples.find((sample) => sample.id === sampleId)?.name ?? t("Audio") }) }
          })
          return <tr key={index} className="border-t">
            {(["start", "end"] as const).map((edge) => <td key={edge} className="p-1"><Input aria-label={message("Range {index} {edge} beat", { index: plainNumber(index + 1), edge })} className="w-24" type="number" min={0} step={0.25} value={segment[edge] / PPQ} disabled={busy} onChange={(event) => { const value = Number(event.target.value); if (Number.isFinite(value)) patch(index, { [edge]: Math.max(0, Math.round(value * PPQ)) }) }} /></td>)}
            <td className="p-1"><Select items={items} value={segment.clip} disabled={busy} onValueChange={(clip: number | null) => { if (clip !== null) patch(index, { clip }) }}><SelectTrigger aria-label={message("Source for range {index}", { index: plainNumber(index + 1) })}><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{items.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}</SelectGroup></SelectContent></Select></td>
            <td className="whitespace-nowrap p-1"><Button size="sm" variant="ghost" disabled={busy || segment.end - segment.start < 2 || segments.length >= 2048} onClick={() => setSegments((rows) => { const middle = Math.floor((segment.start + segment.end) / 2); return [...rows.slice(0, index), { ...segment, end: middle }, { ...segment, start: middle }, ...rows.slice(index + 1)] })}>{t("Split")}</Button><Button size="sm" variant="ghost" aria-label={message("Remove range {index}", { index: plainNumber(index + 1) })} disabled={busy || segments.length < 2} onClick={() => setSegments((rows) => rows.filter((_, i) => i !== index))}>{t("Remove")}</Button></td>
          </tr>
        })}</tbody></table>
      </div>
      <Field orientation="horizontal"><Checkbox id="comp-mute" checked={mute} disabled={busy} onCheckedChange={(value) => setMute(value === true)} /><FieldLabel htmlFor="comp-mute">{t("Mute original source clips")}</FieldLabel></Field>
      {group && <Field orientation="horizontal"><Checkbox id="comp-replace" checked={replace} disabled={busy} onCheckedChange={(value) => setReplace(value === true)} /><FieldLabel htmlFor="comp-replace">{t("Replace the group's previous composite")}</FieldLabel></Field>}
      {group && !options.length && <p role="alert" className="text-destructive">{t("No retained pass is available on every input lane. Restore missing sources or comp individual clips.")}</p>}
      {error && <p role="alert" className="text-destructive">{error}</p>}
      <DialogFooter><Button variant="outline" disabled={busy} onClick={close}>{t("Cancel")}</Button><Button disabled={busy || !segments.length} onClick={() => void createComp()}>{busy ? t("Creating…") : t("Create composite")}</Button></DialogFooter>
    </DialogContent>
  </Dialog>
}
