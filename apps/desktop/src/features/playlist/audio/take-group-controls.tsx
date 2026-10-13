import { useState } from "react"
import type { AudioTakeGroup, AudioTakeLane } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { errorMessage } from "@/lib/ipc"
import { usePlaylistStore } from "../store"
import { selectedAudioClips } from "./ops"
import { openTakeGroupComp } from "./comp-dialog"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const EMPTY: AudioTakeGroup[] = []
export async function groupSelectedAudioTakes() {
  const clips = selectedAudioClips().sort((a, b) => a.id - b.id)
  if (!clips.length || clips.length > 256) return
  const project = useProjectStore.getState().project
  const lanes = new Map<number, AudioTakeLane>()
  for (const clip of clips) {
    if (clip.content.type !== "audio") continue
    const route = clip.content.mixerTrack
    let lane = lanes.get(route)
    if (!lane) {
      const name = project.mixer.tracks.find((track) => track.id === route)?.name ?? "Audio input"
      lane = { name: Array.from(name).filter((character) => character !== "\0").slice(0, 32).join(""), takes: [] }; lanes.set(route, lane)
    }
    lane.takes.push({ pass: lane.takes.length, clip: clip.id })
  }
  await dispatch({ type: "createAudioTakeGroup", name: "Audio takes", lanes: [...lanes.values()] })
}
export function TakeGroupControls() {
  const groups = useProjectStore((state) => state.project.playlist.takeGroups) ?? EMPTY
  const [selected, setSelected] = useState<number | null>(null)
  const [pass, setPass] = useState<number | null>(null)
  const [name, setName] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const group = groups.find((group) => group.id === selected) ?? groups.at(-1)
  const passes = group?.lanes[0]?.takes.filter((take) => group.lanes.every((lane) => lane.takes.some((item) => item.pass === take.pass))) ?? []
  const activePass = passes.some((take) => take.pass === pass) ? pass : passes.at(-1)?.pass ?? null
  const [previousGroup, setPreviousGroup] = useState({ id: group?.id, name: group?.name })
  if (previousGroup.id !== group?.id || previousGroup.name !== group?.name) {
    setPreviousGroup({ id: group?.id, name: group?.name })
    setName(group?.name ?? "")
    setError("")
  }
  const work = async (command: Parameters<typeof dispatch>[0]) => {
    if (busy) return
    setBusy(true); setError("")
    try { if (!await dispatch(command)) setError(t("The take-group action was not applied.")) }
    catch (error) { setError(errorMessage(error)) }
    finally { setBusy(false) }
  }
  return <div className="flex shrink-0 items-center gap-2 border-b px-2 py-1">
    <Popover><PopoverTrigger render={<Button variant="ghost" size="sm" disabled={!groups.length}>{t("Take groups")}{groups.length ? ` (${groups.length})` : ""}</Button>} />
      <PopoverContent align="start" className="w-96 max-w-[90vw]">
        <FieldGroup>
          <Field><FieldLabel>{t("Recording group")}</FieldLabel><Select items={groups.map((group) => ({ value: group.id, label: group.name }))} value={group?.id ?? null} disabled={busy} onValueChange={(id: number | null) => setSelected(id)}><SelectTrigger aria-label={t("Saved recording group")}><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{groups.map((group) => <SelectItem key={group.id} value={group.id}>{group.name}</SelectItem>)}</SelectGroup></SelectContent></Select><FieldDescription>{group?.lanes.length ?? 0}{" "}{t("input lanes ·")}{" "}{passes.length}{" "}{t("common retained passes")}</FieldDescription></Field>
          <Field><FieldLabel htmlFor="take-group-name">{t("Name")}</FieldLabel><Input id="take-group-name" value={name} maxLength={128} disabled={busy} onChange={(event) => setName(event.target.value)} /><div className="flex gap-1"><Button variant="outline" size="sm" disabled={busy || !group} onClick={() => { if (group) void work({ type: "renameAudioTakeGroup", id: group.id, name }) }}>{t("Rename")}</Button><Button variant="ghost" size="sm" disabled={busy || !group} onClick={() => { if (group) void work({ type: "removeAudioTakeGroup", id: group.id }) }}>{t("Ungroup")}</Button></div><FieldDescription>{t("Ungroup removes the association and retains all source and composite clips.")}</FieldDescription></Field>
          <Field><FieldLabel>{t("Pass to audition")}</FieldLabel><Select items={passes.map((take) => ({ value: take.pass, label: message("Pass {pass}", { pass: plainNumber(take.pass + 1) }) }))} value={activePass} disabled={busy || !passes.length} onValueChange={(value: number | null) => setPass(value)}><SelectTrigger aria-label={t("Recording pass")}><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{passes.map((take) => <SelectItem key={take.pass} value={take.pass}>{t("Pass")}{" "}{take.pass + 1}</SelectItem>)}</SelectGroup></SelectContent></Select><div className="flex gap-1"><Button variant="outline" size="sm" disabled={busy || !group || activePass === null} onClick={() => { if (group) void work({ type: "auditionAudioTakeGroup", id: group.id, pass: activePass }) }}>{t("Audition pass")}</Button><Button variant="outline" size="sm" disabled={busy || !group?.comp?.length} onClick={() => { if (group) void work({ type: "auditionAudioTakeGroup", id: group.id, pass: null }) }}>{t("Play composite")}</Button></div><FieldDescription>{t("Switches clip mutes across every input lane together. Use the transport to listen.")}</FieldDescription></Field>
          <div className="flex gap-1"><Button disabled={busy || !group?.lanes.length || !passes.length} onClick={() => { if (group) openTakeGroupComp(group.id) }}>{t("Comp this group…")}</Button><Button variant="ghost" disabled={!group} onClick={() => { if (group) usePlaylistStore.getState().select(group.lanes.flatMap((lane) => lane.takes.map((take) => take.clip))) }}>{t("Select sources")}</Button></div>
        </FieldGroup>
        {error && <p role="alert" className="mt-2 text-destructive">{error}</p>}
      </PopoverContent>
    </Popover>
    {group && <span className="truncate text-xs text-muted-foreground">{group.name}</span>}
  </div>
}
