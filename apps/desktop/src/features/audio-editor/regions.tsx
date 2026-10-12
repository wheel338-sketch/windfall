import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import type { AudioRegion, AudioEditPreview } from "./types"
import type { Selection } from "./selection"

export function Regions({preview,selection,disabled,onSelect,onWrite}: {
  preview: AudioEditPreview; selection: Selection; disabled: boolean
  onSelect(selection: Selection): void; onWrite(regions: AudioRegion[]): void
}) {
  const [name,setName] = useState("Region")
  const regions=preview.regions ?? []
  const canAdd=!disabled && name.trim().length>0 && name.length<=256 && regions.length<256
  function add(marker: boolean) { onWrite([...regions,{name:name.trim(),startFrame:selection.start,endFrame:marker ? selection.start : selection.end}]) }
  return <section aria-label="Audio regions and markers" className="rounded-md border p-2">
    <label htmlFor="editor-region-name" className="text-sm">Region or marker name</label>
    <Input id="editor-region-name" value={name} maxLength={256} disabled={disabled} onChange={event=>setName(event.target.value)} />
    <div className="flex gap-2 py-2"><Button size="sm" variant="outline" disabled={!canAdd} onClick={()=>add(false)}>Add region</Button><Button size="sm" variant="outline" disabled={!canAdd} onClick={()=>add(true)}>Add marker at selection start</Button></div>
    <ul>{regions.map((region,i)=><li key={i} className="flex items-center gap-2">
      <Button size="sm" variant="ghost" disabled={disabled} onClick={()=>onSelect({start:Math.min(region.startFrame,preview.frames-1),end:region.endFrame===region.startFrame ? Math.min(region.endFrame+1,preview.frames) : region.endFrame})}>{region.name} · {region.startFrame}–{region.endFrame}</Button>
      <Button size="sm" variant="ghost" disabled={disabled} aria-label={`Remove ${region.name}`} onClick={()=>onWrite(regions.filter((_,index)=>index!==i))}>Remove</Button>
    </li>)}</ul>
  </section>
}
