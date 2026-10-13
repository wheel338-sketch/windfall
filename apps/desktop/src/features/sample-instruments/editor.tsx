import { useEffect, useId, useRef, useState } from "react"

import type { SampleInstrumentAsset, SampleZone, SoundFontPreset } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { Textarea } from "@/components/ui/textarea"
import type { InstrumentChannel } from "@/lib/channel-source"
import { backend, errorMessage } from "@/lib/ipc"
import { dispatch, useProjectStore, useTransportStore } from "@/lib/store"
import { receivePatch } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { TICKS_PER_STEP } from "@/lib/units"

import { ZoneModulationEditor, DEFAULT_ZONE_MODULATION } from "./zone-modulation-editor"

import { GenericInstrumentEditor } from "../channel-rack/inspector/generic-instrument-editor"

export function newZone(sample: number, key?: number): SampleZone {
  return { sample, keyLow: key ?? 0, keyHigh: key ?? 127, velocityLow: 0, velocityHigh: 127,
    rootKey: key ?? 60, tuneCents: 0, gain: 1, pan: 0, keyFade: 0, velocityFade: 0,
    roundRobin: 0, chokeGroup: 0, loopMode: "off", loopStart: 0, loopEnd: 0,
    attackMs: 0, holdMs: 0, decayMs: 0, sustain: 1, releaseMs: 100, scaleTuning: 100,
    delayMs: 0, modulationGroup: 0, modulation: DEFAULT_ZONE_MODULATION }
}

function ZoneMap({ zones }: { zones: SampleZone[] }) {
  return <svg role="img" aria-label="Zone map: key horizontally, velocity vertically" viewBox="0 0 128 128" className="h-32 w-full rounded border bg-muted">
    {zones.map((zone, index) => <rect key={index} x={zone.keyLow} y={127-zone.velocityHigh}
      width={zone.keyHigh-zone.keyLow+1} height={zone.velocityHigh-zone.velocityLow+1}
      className="fill-primary/20 stroke-primary" strokeWidth="0.5" />)}
  </svg>
}

function NumberEdit({ label, value, min = 0, max = 127, commit }: {
  label: string; value: number; min?: number; max?: number; commit: (value: number) => void
}) {
  return <Input key={String(value)} aria-label={label} type="number" min={min} max={max} step="any"
    defaultValue={value} onBlur={(event) => {
      const next = event.currentTarget.valueAsNumber
      if (Number.isFinite(next) && next !== value) commit(next)
    }} />
}

function PadSteps({ channel, pad }: { channel: InstrumentChannel; pad: number }) {
  const patternId = useTransportStore(state => state.pattern)
  const pattern = useProjectStore(state => state.project.patterns.find(item => item.id === patternId))
  const [page, setPage] = useState(0)
  if (!pattern) return null
  const start = Math.min(page * 16, Math.floor((pattern.lengthSteps - 1) / 16) * 16)
  const notes = pattern.lanes.find(lane => lane.channel === channel.id)?.notes ?? []
  return <FieldGroup>
    <FieldDescription>Pad {pad - 35} in {pattern.name}: steps {start + 1}–{Math.min(start + 16, pattern.lengthSteps)}. Other pad notes stay in place.</FieldDescription>
    <div role="group" aria-label={`Pad ${pad - 35} pattern steps`} className="grid grid-cols-8 gap-1">
      {Array.from({length:Math.min(16,pattern.lengthSteps-start)},(_,index)=>{
        const step=start+index
        const hits=notes.filter(note=>note.key===pad && Math.floor(note.start/TICKS_PER_STEP)===step)
        return <Button key={step} size="sm" variant={hits.length ? "default" : "outline"} aria-label={`Pad ${pad-35} step ${step+1}`} aria-pressed={hits.length>0}
          onClick={()=>void dispatch(hits.length ? {type:"removeNotes",pattern:pattern.id,channel:channel.id,notes:hits.map(note=>note.id)} :
            {type:"addNotes",pattern:pattern.id,channel:channel.id,notes:[{start:step*TICKS_PER_STEP,length:TICKS_PER_STEP,key:pad,velocity:1}]})}>{step+1}</Button>
      })}
    </div>
    <div className="flex gap-1"><Button variant="outline" disabled={start===0} onClick={()=>setPage(Math.max(0,start/16-1))}>Previous pad steps</Button>
      <Button variant="outline" disabled={start+16>=pattern.lengthSteps} onClick={()=>setPage(start/16+1)}>Next pad steps</Button></div>
  </FieldGroup>
}

function ZonesEditor({ channel, pads }: { channel: InstrumentChannel; pads: boolean }) {
  const samples = useProjectStore((state) => state.project.samples)
  const zones = channel.sampleInstrument?.type === "zones" ? channel.sampleInstrument.zones : []
  const [sample, setSample] = useState<number | null>(null)
  const [pad, setPad] = useState(36)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const source = sample ?? samples[0]?.id ?? null
  const replace = (next: SampleZone[]) => void dispatch({ type: "editSampleZones", channel: channel.id, expected: zones, zones: next })
  const change = (index: number, patch: Partial<SampleZone>) => replace(zones.map((zone, i) => i === index ? { ...zone, ...patch } : zone))
  const visible = zones.map((zone,index) => ({ zone,index })).filter(({zone}) => !pads || zone.keyLow === pad)
  return <FieldGroup>
    {pads ? <div role="group" aria-label="Sixteen drum pads" className="grid grid-cols-4 gap-1">
      {Array.from({length:16},(_,index) => {
        const key = 36+index
        return <Button key={key} variant={pad === key ? "default" : "outline"} aria-pressed={pad === key}
          onClick={() => setPad(key)} onPointerDown={() => { void backend.auditionNoteOn(channel.id,key,1) }}
          onPointerUp={() => { void backend.auditionNoteOff(channel.id,key) }}
          onPointerCancel={() => { void backend.auditionNoteOff(channel.id,key) }}
          onPointerLeave={() => { void backend.auditionNoteOff(channel.id,key) }}>
          Pad {index+1} · {zones.filter(zone=>zone.keyLow===key).length} layers
        </Button>
      })}
    </div> : <ZoneMap zones={zones} />}
    {pads && <PadSteps channel={channel} pad={pad} />}
    <Field><FieldLabel>Source sample</FieldLabel>
      <Select value={source === null ? "" : String(source)} onValueChange={(value) => setSample(Number(value))}>
        <SelectTrigger aria-label="Source sample"><SelectValue /></SelectTrigger>
        <SelectContent><SelectGroup>{samples.map(item=><SelectItem key={item.id} value={String(item.id)}>{item.name}</SelectItem>)}</SelectGroup></SelectContent>
      </Select>
      <FieldDescription>Choose an existing project source or import audio directly as one undo step.</FieldDescription>
    </Field>
    <Button variant="outline" disabled={busy || !backend.sampleInstrumentImportZone || zones.length >= 256} onClick={async () => {
      const generation = getProjectGeneration()
      setBusy(true)
      try {
        const file = await backend.pickAudioFile()
        if (file && generation === getProjectGeneration()) {
          const result = await backend.sampleInstrumentImportZone!(channel.id, channel.sampleInstrument ?? null, file, pads ? pad : null)
          if (generation === getProjectGeneration()) { receivePatch(result.patch); setError(null) }
        }
      } catch(error) { if(generation === getProjectGeneration())setError(errorMessage(error)) }
      finally { setBusy(false) }
    }}>{busy ? "Importing…" : pads ? "Import pad source" : "Import zone source"}</Button>
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    <Button disabled={source === null || zones.length >= 256} onClick={() => { if(source!==null) replace([...zones,newZone(source,pads?pad:undefined)]) }}>
      {pads ? "Add pad layer" : "Add zone"}
    </Button>
    <Table><TableHeader><TableRow><TableHead>Source</TableHead><TableHead>Keys</TableHead><TableHead>Velocity</TableHead><TableHead>Mapping</TableHead><TableHead>Actions</TableHead></TableRow></TableHeader>
      <TableBody>{visible.map(({zone,index})=><TableRow key={index}>
        <TableCell>{samples.find(item=>item.id===zone.sample)?.name ?? "Missing source"}</TableCell>
        <TableCell>
          {!pads && <><NumberEdit label={`Zone ${index+1} low key`} value={zone.keyLow} commit={value=>change(index,{keyLow:value})} />
            <NumberEdit label={`Zone ${index+1} high key`} value={zone.keyHigh} commit={value=>change(index,{keyHigh:value})} /></>}
          <NumberEdit label={`Zone ${index+1} root key`} value={zone.rootKey} commit={value=>change(index,{rootKey:value})} />
        </TableCell>
        <TableCell><NumberEdit label={`Zone ${index+1} low velocity`} value={zone.velocityLow} commit={value=>change(index,{velocityLow:value})} />
          <NumberEdit label={`Zone ${index+1} high velocity`} value={zone.velocityHigh} commit={value=>change(index,{velocityHigh:value})} /></TableCell>
        <TableCell>
          <NumberEdit label={`Zone ${index+1} round robin`} value={zone.roundRobin} max={255} commit={value=>change(index,{roundRobin:value})} />
          <NumberEdit label={`Zone ${index+1} choke group`} value={zone.chokeGroup} max={255} commit={value=>change(index,{chokeGroup:value})} />
          <NumberEdit label={`Zone ${index+1} key fade`} value={zone.keyFade} commit={value=>change(index,{keyFade:value})} />
          <NumberEdit label={`Zone ${index+1} velocity fade`} value={zone.velocityFade} commit={value=>change(index,{velocityFade:value})} />
          <NumberEdit label={`Zone ${index+1} gain`} value={zone.gain} max={4} commit={value=>change(index,{gain:value})} />
          <NumberEdit label={`Zone ${index+1} pan`} value={zone.pan} min={-1} max={1} commit={value=>change(index,{pan:value})} />
          <NumberEdit label={`Zone ${index+1} tuning cents`} value={zone.tuneCents} min={-12000} max={12000} commit={value=>change(index,{tuneCents:value})} />
          <Select value={zone.loopMode} onValueChange={(value)=>{if(value==="off"||value==="continuous"||value==="untilRelease")change(index,{loopMode:value})}}>
            <SelectTrigger aria-label={`Zone ${index+1} loop mode`}><SelectValue /></SelectTrigger>
            <SelectContent><SelectGroup><SelectItem value="off">One-shot</SelectItem><SelectItem value="continuous">Continuous loop</SelectItem><SelectItem value="untilRelease">Loop until release</SelectItem></SelectGroup></SelectContent>
          </Select>
          <NumberEdit label={`Zone ${index+1} loop start`} value={zone.loopStart} max={4294967295} commit={value=>change(index,{loopStart:value})} />
          <NumberEdit label={`Zone ${index+1} loop end`} value={zone.loopEnd} max={4294967295} commit={value=>change(index,{loopEnd:value})} />
          {!pads && <>
            <NumberEdit label={`Zone ${index+1} modulation group`} value={zone.modulationGroup??0} max={255} commit={value=>change(index,{modulationGroup:value})} />
            <ZoneModulationEditor zone={{...zone,modulation: zone.modulationGroup ? zones.find(item=>item.modulationGroup===zone.modulationGroup)?.modulation??DEFAULT_ZONE_MODULATION : zone.modulation}} index={index}
              commit={modulation=>replace(zones.map((item,i)=>i===index || (zone.modulationGroup && item.modulationGroup===zone.modulationGroup) ? {...item,modulation} : item))} />
          </>}
          <NumberEdit label={`Zone ${index+1} delay ms`} value={zone.delayMs??0} max={100000} commit={value=>change(index,{delayMs:value})} />
          <NumberEdit label={`Zone ${index+1} attack ms`} value={zone.attackMs} max={100000} commit={value=>change(index,{attackMs:value})} />
          <NumberEdit label={`Zone ${index+1} hold ms`} value={zone.holdMs} max={100000} commit={value=>change(index,{holdMs:value})} />
          <NumberEdit label={`Zone ${index+1} scale tuning`} value={zone.scaleTuning} max={1200} commit={value=>change(index,{scaleTuning:value})} />
          <NumberEdit label={`Zone ${index+1} decay ms`} value={zone.decayMs} max={100000} commit={value=>change(index,{decayMs:value})} />
          <NumberEdit label={`Zone ${index+1} sustain`} value={zone.sustain} max={1} commit={value=>change(index,{sustain:value})} />
          <NumberEdit label={`Zone ${index+1} release ms`} value={zone.releaseMs} max={100000} commit={value=>change(index,{releaseMs:value})} />

        </TableCell>
        <TableCell><Button variant="outline" onClick={()=>replace(zones.filter((_,i)=>i!==index))}>Remove zone {index+1}</Button></TableCell>
      </TableRow>)}</TableBody>
    </Table>
    <BankSourceRecovery channel={channel} />
    <PortableBank channel={channel} />
    {pads && <FieldDescription>Pad notes are keys 36–51. Add these keys in the piano roll to sequence all layers and chokes in a pattern.</FieldDescription>}
  </FieldGroup>
}

function PortableBank({ channel }: { channel: InstrumentChannel }) {
  const [path,setPath] = useState("")
  const [busy,setBusy] = useState(false)
  const [error,setError] = useState<string|null>(null)
  const [saved,setSaved] = useState(false)
  return <FieldGroup>
    <Field><FieldLabel>Bank file (.wfbank)</FieldLabel><Input aria-label="Bank file" value={path} onChange={event=>{setPath(event.target.value);setSaved(false)}} />
      <FieldDescription>Windfall bank files bring their own referenced audio. They can be moved with the adjacent source folder.</FieldDescription></Field>
    <div className="flex gap-1">
      <Button disabled={busy || !path || !backend.sampleInstrumentLoadBank} onClick={async()=>{
        const generation=getProjectGeneration();setBusy(true)
        try { const result=await backend.sampleInstrumentLoadBank!(channel.id,channel.sampleInstrument??null,path)
          if(generation===getProjectGeneration()){receivePatch(result.patch);setError(null)} }
        catch(error){if(generation===getProjectGeneration())setError(errorMessage(error))}finally{setBusy(false)}
      }}>Load instrument bank</Button>
      <Button variant="outline" disabled={busy || !path || channel.sampleInstrument?.type!=="zones" || !backend.sampleInstrumentSaveBank} onClick={async()=>{
        const generation=getProjectGeneration();setBusy(true)
        try { await backend.sampleInstrumentSaveBank!(channel.id,channel.sampleInstrument??null,path)
          if(generation===getProjectGeneration()){setSaved(true);setError(null)} }
        catch(error){if(generation===getProjectGeneration())setError(errorMessage(error))}finally{setBusy(false)}
      }}>Save instrument bank</Button>
    </div>
    {saved && <FieldDescription role="status">Instrument bank saved.</FieldDescription>}
    {!backend.sampleInstrumentLoadBank && <FieldDescription>Portable bank files require the desktop app.</FieldDescription>}
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </FieldGroup>
}

function BankSourceRecovery({ channel }: { channel: InstrumentChannel }) {
  const samples = useProjectStore(state => state.project.samples)
  const [missing,setMissing] = useState<number[]>([])
  const [busy,setBusy] = useState(false)
  const [error,setError] = useState<string|null>(null)
  const ids = channel.sampleInstrument?.type === "zones" ? [...new Set(channel.sampleInstrument.zones.map(zone=>zone.sample))] : []
  useEffect(()=>{
    let current=true
    void backend.sampleInstrumentMissingSources?.(channel.id).then(ids=>{if(current)setMissing(ids)})
      .catch(error=>{if(current)setError(errorMessage(error))})
    return ()=>{current=false}
  },[channel.id,channel.sampleInstrument,samples])
  return <FieldGroup>
    {missing.length>0 && <Alert><AlertDescription>Missing sample-bank sources. Relink each source to recover this instrument; its mapping and source IDs are preserved.</AlertDescription></Alert>}
    {ids.map(id=>{
      const source = samples.find(sample=>sample.id===id)
      return source && <div key={id} className="flex items-center gap-2">
        <FieldDescription>{missing.includes(id) ? "Missing source: " : "Source: "}{source.name}</FieldDescription>
        <Button variant="outline" disabled={busy || !backend.sampleInstrumentRelinkSource} onClick={async()=>{
          const generation=getProjectGeneration();setBusy(true)
          try {
            const path=await backend.pickAudioFile()
            if(path && generation===getProjectGeneration()) {
              const result=await backend.sampleInstrumentRelinkSource!(channel.id,channel.sampleInstrument??null,id,source.path,path)
              if(generation===getProjectGeneration()){receivePatch(result.patch);setMissing(ids=>ids.filter(item=>item!==id));setError(null)}
            }
          } catch(error){if(generation===getProjectGeneration())setError(errorMessage(error))}finally{setBusy(false)}
        }}>Relink source {source.name}</Button>
      </div>
    })}
    {!backend.sampleInstrumentRelinkSource && ids.length>0 && <FieldDescription>Source relinking requires the desktop app.</FieldDescription>}
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </FieldGroup>
}

function PlayerEditor({ channel }: { channel: InstrumentChannel }) {
  const id = useId()
  const [json,setJson] = useState("")
  const [error,setError] = useState<string | null>(null)
  const zones = channel.sampleInstrument?.type === "zones" ? channel.sampleInstrument.zones : []
  return <FieldGroup>
    <ZoneMap zones={zones} />
    <BankSourceRecovery channel={channel} />
    <PortableBank channel={channel} />
    <Field><FieldLabel htmlFor={id}>Ready-made bank references</FieldLabel>
      <Textarea id={id} value={json} onChange={event=>setJson(event.target.value)} />
      <FieldDescription>Load a zone-bank JSON document using sample IDs present in this project. This player does not edit its mapping.</FieldDescription>
    </Field>
    <Button onClick={()=>{
      try {
        const parsed: unknown = JSON.parse(json)
        if (!parsed || typeof parsed !== "object" || !("type" in parsed) || parsed.type!=="zones" || !("zones" in parsed) || !Array.isArray(parsed.zones)) throw new Error("Expected a zone-bank document")
        // Rust validates every reference, numeric bound and zone field.
        void dispatch({type:"setSampleInstrumentAsset",channel:channel.id,expected:channel.sampleInstrument??null,asset:parsed as SampleInstrumentAsset})
        setError(null)
      } catch(error) { setError(errorMessage(error)) }
    }}>Load bank</Button>
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </FieldGroup>
}

function SoundFontEditor({ channel }: { channel: InstrumentChannel }) {
  const selected = channel.sampleInstrument?.type === "soundFont" ? channel.sampleInstrument : null
  const samples = useProjectStore(state => state.project.samples)
  const source = samples.find(sample => sample.id === selected?.source)
  const storedPath = source?.path ?? selected?.path
  const savedPath = storedPath?.path ?? ""
  const [draft,setDraft] = useState({ savedPath, value: savedPath })
  const path = draft.savedPath === savedPath ? draft.value : savedPath
  const [presets,setPresets] = useState<SoundFontPreset[]>([])
  const [error,setError] = useState<string | null>(null)
  const [busy,setBusy] = useState(false)
  const [missing,setMissing] = useState(false)
  const [relink,setRelink] = useState(false)
  const [replacement,setReplacement] = useState("")
  const request = useRef({ value: 0 })
  useEffect(() => {
    const ticket = request.current
    let current = true
    void backend.sampleInstrumentMissingSources?.(channel.id).then(ids => {
      if (current) setMissing(selected?.source != null && ids.includes(selected.source))
    }).catch(error => { if (current) setError(errorMessage(error)) })
    return () => { current = false; ticket.value++ }
  }, [channel.id, selected, storedPath])
  return <FieldGroup>
    <Field><FieldLabel>SoundFont file</FieldLabel><Input aria-label="SoundFont file" value={path} onChange={event=>{setDraft({ savedPath, value: event.target.value });setPresets([]);request.current.value++}} /></Field>
    <Button disabled={busy || !backend.soundfontPresets || !path} onClick={async()=>{
      const token = ++request.current.value
      try { const found=await backend.soundfontPresets!(path);if(token===request.current.value){setPresets(found);setError(null)} }
      catch(error){if(token===request.current.value)setError(errorMessage(error))}
    }}>Browse presets</Button>
    <div role="group" aria-label="SoundFont presets" className="flex flex-col gap-1">
      {presets.map(preset=><Button key={`${preset.bank}:${preset.program}`} variant="outline"
        disabled={busy || !backend.soundfontLoad} onClick={async()=>{
          const generation=getProjectGeneration();setBusy(true)
          try {
            const result=await backend.soundfontLoad!(channel.id,selected,path,preset.bank,preset.program)
            if(generation===getProjectGeneration()){receivePatch(result.patch);setError(null)}
          } catch(error){if(generation===getProjectGeneration())setError(errorMessage(error))}
          finally{setBusy(false)}
        }}>{preset.bank}:{preset.program} · {preset.name}</Button>)}
    </div>
    {selected && <FieldDescription>Selected bank {selected.bank}, program {selected.program}</FieldDescription>}
    {source && <FieldDescription>Project source {source.id}: {source.name}. Portable project archives include this bank.</FieldDescription>}
    {missing && <Alert><AlertDescription>Missing SoundFont file. Relink its project source to restore playback.</AlertDescription></Alert>}
    {source && <Button variant="outline" disabled={busy || !backend.soundfontRelink} onClick={()=>{setReplacement("");setError(null);setRelink(true)}}>Relink SoundFont source</Button>}
    <Dialog open={relink} onOpenChange={setRelink}>
      <DialogContent>
        <DialogTitle>Relink SoundFont source</DialogTitle>
        <DialogDescription>Choose a replacement SF2 file. Windfall validates every selected preset using this source before updating its path. The source identity is preserved and the change can be undone.</DialogDescription>
        <Input aria-label="Replacement SoundFont file" value={replacement} onChange={event=>setReplacement(event.target.value)} />
        <Button disabled={busy || !replacement || !source || !selected || !backend.soundfontRelink} onClick={async()=>{
          if(!source || !selected)return
          const generation=getProjectGeneration();setBusy(true)
          try {
            const result=await backend.soundfontRelink!(channel.id,selected,source.path,replacement)
            if(generation===getProjectGeneration()){receivePatch(result.patch);setRelink(false);setMissing(false);setError(null)}
          } catch(error){if(generation===getProjectGeneration())setError(errorMessage(error))}
          finally{setBusy(false)}
        }}>Verify and relink SoundFont</Button>
        {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
      </DialogContent>
    </Dialog>
    {!backend.soundfontLoad && <Alert><AlertDescription>SoundFont file loading requires the desktop app.</AlertDescription></Alert>}
    {error && !relink && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </FieldGroup>
}

function SpeechEditor({ channel }: { channel: InstrumentChannel }) {
  const recipe=channel.sampleInstrument?.type==="speech" ? channel.sampleInstrument : null
  const [text,setText]=useState(recipe?.text??"")
  const [pitch,setPitch]=useState(recipe?.pitch??48)
  const [duration,setDuration]=useState(recipe?.duration??2)
  const [busy,setBusy]=useState(false)
  const job = useRef<{request: number | null; cancelled: boolean} | null>(null)
  const cancel = () => {
    const active = job.current
    if (!active) return
    active.cancelled = true
    if (active.request !== null) void backend.sampleInstrumentSpeechCancel?.(channel.id, active.request)
      .catch(error => setError(errorMessage(error)))
  }
  useEffect(() => () => {
    const active = job.current
    if (active) {
      active.cancelled = true
      if (active.request !== null) void backend.sampleInstrumentSpeechCancel?.(channel.id, active.request)
    }
  }, [channel.id])
  const [error,setError]=useState<string|null>(null)
  return <FieldGroup>
    <Field><FieldLabel>Text</FieldLabel><Textarea aria-label="Text" maxLength={512} value={text} onChange={event=>setText(event.target.value)} /></Field>
    <Field><FieldLabel>Voice pitch (MIDI key)</FieldLabel><Input aria-label="Voice pitch" type="number" min={24} max={84} value={pitch} onChange={event=>setPitch(event.target.valueAsNumber)} /></Field>
    <Field><FieldLabel>Duration (seconds)</FieldLabel><Input aria-label="Speech duration" type="number" min={0.1} max={60} step={0.1} value={duration} onChange={event=>setDuration(event.target.valueAsNumber)} /></Field>
    <Button disabled={busy || !backend.sampleInstrumentSpeech || !text.trim()} onClick={async()=>{
      const generation = getProjectGeneration()
      const active = {request: null as number | null, cancelled: false}
      job.current = active
      setBusy(true)
      try {
        if (backend.sampleInstrumentSpeechBegin) {
          active.request = await backend.sampleInstrumentSpeechBegin(channel.id)
          if (active.cancelled) await backend.sampleInstrumentSpeechCancel?.(channel.id, active.request)
        }
        const result = await backend.sampleInstrumentSpeech!(channel.id,channel.sampleInstrument??null,text,pitch,duration,active.request ?? undefined)
        if (generation===getProjectGeneration()) { receivePatch(result.patch); setError(null) }
      } catch(error) { if(generation===getProjectGeneration())setError(errorMessage(error)) }
      finally { if(job.current===active)job.current=null;setBusy(false) }
    }}>{busy ? "Rendering…" : "Render and save source"}</Button>
    {busy && <Button variant="outline" disabled={!backend.sampleInstrumentSpeechCancel} onClick={cancel}>Cancel speech rendering</Button>}
    <FieldDescription>Own robotic English rule voice. Rendering saves audio for project reopen and undo. Notes transpose the saved phrase from key 60.</FieldDescription>
    {!backend.sampleInstrumentSpeech && <Alert><AlertDescription>Saved speech rendering requires the desktop app.</AlertDescription></Alert>}
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </FieldGroup>
}

export function SampleInstrumentEditor({ channel }: { channel: InstrumentChannel }) {
  const kind=channel.source.params.type
  return <div className="flex flex-col gap-3">
    {kind==="multiSampler" && <ZonesEditor channel={channel} pads={false} />}
    {kind==="padBank" && <ZonesEditor channel={channel} pads />}
    {kind==="samplePlayer" && <PlayerEditor channel={channel} />}
    {kind==="soundFont" && <SoundFontEditor key={channel.id} channel={channel} />}
    {kind==="textVoice" && <SpeechEditor key={channel.id} channel={channel} />}
    {kind==="pianoKeys" && <FieldGroup>
      <FieldDescription>Current source: {channel.sampleInstrument?.type==="piano" && channel.sampleInstrument.electric ? "Electric keyboard" : "Struck-string piano"}</FieldDescription>
      <Button variant="outline" onClick={()=>void dispatch({type:"setSampleInstrumentAsset",channel:channel.id,expected:channel.sampleInstrument??null,
        asset:{type:"piano",electric:!(channel.sampleInstrument?.type==="piano" && channel.sampleInstrument.electric),version:1}})}>Switch piano / electric keyboard</Button>
      <FieldDescription>Windfall-generated struck-string and electric-keyboard sources, with two velocity layers and fifteen root keys.</FieldDescription>
    </FieldGroup>}
    <GenericInstrumentEditor channel={channel.id} params={channel.source.params} />
  </div>
}
