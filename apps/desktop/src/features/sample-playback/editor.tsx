import { useCallback, useEffect, useState } from "react"
import type { InstrumentParams, SampleInfo, SamplePlayback, SliceRegion } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { useParamAutomation } from "@/features/automation/live"
import { instrumentDescriptor, ParamControl, useParamBinding } from "@/features/params"
import type { SliceOptions } from "@/features/slicer/types"
import type { InstrumentChannel } from "@/lib/channel-source"
import { backend, errorMessage } from "@/lib/ipc"
import { dispatch, useProjectStore, useTransportStore } from "@/lib/store"
import { receivePatch } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { CurveEditor } from "./curve-editor"
import { defaultAsset } from "./settings"

function NumberField({ label, value, min, max, step = 0.01, set }: {
  label: string; value: number; min: number; max: number; step?: number; set(value: number): void
}) {
  return <Field orientation="horizontal"><FieldLabel>{label}</FieldLabel><Input key={value} aria-label={label} type="number"
    defaultValue={value} min={min} max={max} step={step} onBlur={(event) => {
      const next = Number(event.target.value)
      if (Number.isFinite(next) && next >= min && next <= max && next !== value) set(next)
    }} /></Field>
}

function PlaybackControls({ channel, params }: { channel: number; params: InstrumentParams }) {
  const setParam = useCallback((param: number, value: number, gesture?: number) =>
    dispatch({ type: "setInstrumentParam", channel, param, value }, gesture), [channel])
  const automation = useParamAutomation((param) => ({ type: "instrumentParam", channel, param }), String(channel))
  const descriptor = instrumentDescriptor(params.type)
  const bind = useParamBinding({ descriptor, params, setParam, ...automation })
  return <div role="group" aria-label={params.type === "grainCloud" ? "Grain controls" : "Playback controls"} className="flex flex-wrap gap-2">
    {descriptor.params.map((info) => <ParamControl key={info.id} {...bind(info.id)} />)}
  </div>
}

/** Purpose-built immutable source, slice map and per-note curve editor. */
export function SamplePlaybackEditor({ channel }: { channel: InstrumentChannel }) {
  const params = channel.source.params
  const asset = channel.samplePlayback ?? defaultAsset()
  const samples = useProjectStore((state) => state.project.samples)
  const patternIds = useProjectStore((state) => state.project.patterns)
  const selectedPattern = useTransportStore((state) => state.pattern)
  const [selected, select] = useState(0)
  const [curveKey, setCurveKey] = useState<number | null>(null)
  const [info, setInfo] = useState<{ sample: number; info: SampleInfo } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [sensitivity, setSensitivity] = useState(0.6)
  const slicing = params.type === "sliceMap" || params.type === "sliceDeck"
  const availableKeys = params.type === "sliceMap" || params.type === "sliceDeck" ? 128 - params.baseKey : 128
  const index = Math.min(selected, asset.slices.length - 1)
  const region = asset.slices[index]
  const save = (next: SamplePlayback) => { void dispatch({ type: "setSamplePlayback", channel: channel.id, asset: { ...next, legacyKeyWrap: false } }) }
  const setRegion = (patch: Partial<SliceRegion>) => save({ ...asset, slices: asset.slices.map((s, i) => i === index ? { ...s, ...patch } : s) })
  useEffect(() => {
    let alive = true
    const generation = getProjectGeneration()
    const sample = asset.sample
    if (sample != null) void backend.sampleInfoById(sample).then((value) => {
      if (alive && generation === getProjectGeneration()) setInfo({ sample, info: value })
    }).catch((failure) => { if (alive && generation === getProjectGeneration()) setError(errorMessage(failure)) })
    return () => { alive = false }
  }, [asset.sample])
  const detect = async (options: SliceOptions) => {
    const generation = getProjectGeneration()
    setBusy(true); setError(null)
    try {
      const guard = await backend.timelineState()
      if (generation !== getProjectGeneration()) return
      const result = await backend.samplePlaybackDetect(channel.id, options, guard.generation, guard.revision)
      if (generation === getProjectGeneration()) { receivePatch(result.patch); select(0) }
    } catch (failure) { if (generation === getProjectGeneration()) setError(errorMessage(failure)) }
    finally { if (generation === getProjectGeneration()) setBusy(false) }
  }
  const peaks = info?.sample === asset.sample ? info.info.peaks : []
  const exportSource = async () => {
    const generation = getProjectGeneration()
    setBusy(true); setError(null)
    try {
      const guard = await backend.timelineState()
      if (generation === getProjectGeneration()) await backend.samplePlaybackExport(channel.id, guard.generation, guard.revision)
    } catch (failure) { if (generation === getProjectGeneration()) setError(errorMessage(failure)) }
    finally { if (generation === getProjectGeneration()) setBusy(false) }
  }
  const waveform = peaks.reduce((path, _, i) => i % 2 ? path : `${path} M${i / peaks.length * 300},${35 - peaks[i] * 30} V${35 - peaks[i + 1] * 30}`, "")
  const curve = curveKey === null ? asset.curve : asset.keyCurves.find((c) => c.key === curveKey)?.curve ?? asset.curve
  return <div className="flex flex-col gap-3" aria-label="Sample playback editor">
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    <FieldGroup>
      {[0, ...(params.type === "sliceDeck" ? [1] : [])].map((deck) => <Field key={deck}>
        <FieldLabel>Source {deck === 0 ? "A" : "B"}</FieldLabel>
        <Select<string> items={[{ value: "none", label: "No source" }, ...samples.map(sample => ({ value: String(sample.id), label: sample.name }))]} value={String((deck ? asset.sampleB : asset.sample) ?? "none")} onValueChange={(value) => {
          if (value === null) return
          const source = value === "none" ? null : Number(value)
          save({ ...asset, ...(deck ? { sampleB: source } : { sample: source, inlineSource: undefined }) })
        }}><SelectTrigger aria-label={`Source ${deck === 0 ? "A" : "B"}`}><SelectValue /></SelectTrigger>
          <SelectContent><SelectGroup><SelectItem value="none">No source</SelectItem>
            {samples.map((sample) => <SelectItem key={sample.id} value={String(sample.id)}>{sample.name}</SelectItem>)}
          </SelectGroup></SelectContent>
        </Select>
      </Field>)}
    </FieldGroup>
    <p className="text-xs text-muted-foreground">Choose an imported sample from the project. Browser mode uses simulated audio fixtures.</p>
    {asset.inlineSource && asset.sample == null && <p className="text-xs text-muted-foreground">The migrated source is retained inside this project. Choosing Source A replaces it.</p>}
    <PlaybackControls channel={channel.id} params={params} />
    {params.type === "grainCloud" && <p className="text-xs text-muted-foreground">Grain spacing is 1000 ÷ density milliseconds. Size controls overlap; spray randomizes source positions. The same seed reproduces the schedule after reset.</p>}
    {slicing && <>
      {asset.slices.length > availableKeys && <Alert><AlertDescription>
        This map extends past MIDI key 127. Lower Base key to trigger every slice and generate its pattern.
      </AlertDescription></Alert>}
      <svg viewBox="0 0 300 70" role="img" aria-label="Source waveform and slice boundaries" className="h-24 w-full rounded border bg-muted">
        <path d={waveform} stroke="currentColor" strokeWidth="1" />
        {asset.slices.map((s, i) => <rect key={i} x={s.start * 300} y={0} width={(s.end - s.start) * 300} height={70}
          fill="currentColor" fillOpacity={i === index ? 0.2 : 0.04} stroke="currentColor" strokeOpacity={0.5} onClick={() => select(i)} />)}
      </svg>
      <FieldGroup>
        <NumberField label="Source tempo" value={asset.sourceBpm} min={10} max={522} set={(sourceBpm) => save({ ...asset, sourceBpm })} />
        <NumberField label="Source beats" value={asset.sourceBeats} min={0.001} max={256} set={(sourceBeats) => save({ ...asset, sourceBeats })} />
        <NumberField label="Transient sensitivity" value={sensitivity} min={0} max={1} set={setSensitivity} />
      </FieldGroup>
      <div className="flex flex-wrap gap-2">
        <Button size="sm" disabled={busy || asset.sample == null} onClick={() => { void detect({ mode: "transients", sensitivity }) }}>Detect transients</Button>
        <Button size="sm" variant="outline" disabled={busy || asset.sample == null} onClick={() => { void detect({ mode: "grid", gridTicks: 960 }) }}>Detect beat grid</Button>
        <Button size="sm" variant="outline" disabled={busy || asset.sample == null} onClick={() => { void detect({ mode: "wavMarkers" }) }}>Import WAV slices</Button>
        <Button size="sm" variant="outline" disabled={busy || asset.sample == null} onClick={() => { void exportSource() }}>Export source WAV with slices</Button>
        <Button size="sm" variant="outline" disabled={!channel.samplePlayback || asset.slices.length > availableKeys || patternIds.length === 0} onClick={() => { void dispatch({ type: "generateSlicePattern", channel: channel.id,
          pattern: patternIds.some((p) => p.id === selectedPattern) ? selectedPattern : patternIds[0].id }) }}>Generate slice pattern</Button>
      </div>
      <FieldGroup><Field><FieldLabel>Selected slice</FieldLabel><Select<string> value={String(index)} onValueChange={(v) => { if (v != null) select(Number(v)) }}>
        <SelectTrigger aria-label="Selected slice"><SelectValue /></SelectTrigger><SelectContent><SelectGroup>
          {asset.slices.map((_, i) => <SelectItem key={i} value={String(i)}>Slice {i + 1} · key {(params.type === "sliceMap" || params.type === "sliceDeck" ? params.baseKey : 60) + i}</SelectItem>)}
        </SelectGroup></SelectContent></Select></Field>
        <NumberField label="Slice start" value={region.start} min={0} max={region.end - 0.00001} set={(start) => setRegion({ start })} />
        <NumberField label="Slice end" value={region.end} min={region.start + 0.00001} max={1} set={(end) => setRegion({ end })} />
        {params.type === "sliceDeck" && <>
          <NumberField label="Slice gain" value={region.gain} min={0} max={2} set={(gain) => setRegion({ gain })} />
          <NumberField label="Slice pitch" value={region.pitch} min={-48} max={48} set={(pitch) => setRegion({ pitch })} />
          <NumberField label="Slice attack" value={region.attackMs} min={0} max={1000} set={(attackMs) => setRegion({ attackMs })} />
          <NumberField label="Slice release" value={region.releaseMs} min={1} max={1000} set={(releaseMs) => setRegion({ releaseMs })} />
          <NumberField label="Slice deck" value={region.deck} min={0} max={1} step={1} set={(deck) => setRegion({ deck })} />
          <Field orientation="horizontal"><FieldLabel>Reverse slice</FieldLabel><Switch aria-label="Reverse slice" checked={region.reverse} onCheckedChange={(reverse) => setRegion({ reverse })} /></Field>
        </>}
      </FieldGroup>
      <div className="flex flex-wrap gap-2">
        <Button size="sm" variant="outline" disabled={asset.slices.length >= 128} onClick={() => {
          const midpoint = (region.start + region.end) / 2
          const slices = [...asset.slices]; slices.splice(index, 1, { ...region, end: midpoint }, { ...region, start: midpoint }); save({ ...asset, slices })
        }}>Split slice</Button>
        <Button size="sm" variant="outline" disabled={asset.slices.length <= 1} onClick={() => save({ ...asset, slices: asset.slices.filter((_, i) => i !== index) })}>Delete slice</Button>
        {[-1, 1].map((delta) => <Button key={delta} size="sm" variant="outline" disabled={index + delta < 0 || index + delta >= asset.slices.length} onClick={() => {
          const slices = [...asset.slices]; [slices[index], slices[index + delta]] = [slices[index + delta], slices[index]]; save({ ...asset, slices }); select(index + delta)
        }}>Move slice {delta < 0 ? "earlier" : "later"}</Button>)}
      </div>
    </>}
    {params.type === "waveRide" && <FieldGroup>
      <Field><FieldLabel>Curve note</FieldLabel><Select<string> value={curveKey === null ? "shared" : String(curveKey)} onValueChange={(v) => { if (v != null) setCurveKey(v === "shared" ? null : Number(v)) }}>
        <SelectTrigger aria-label="Curve note"><SelectValue /></SelectTrigger><SelectContent><SelectGroup><SelectItem value="shared">Shared curve</SelectItem>
          {Array.from({ length: 128 }, (_, key) => <SelectItem key={key} value={String(key)}>Key {key}</SelectItem>)}
        </SelectGroup></SelectContent></Select></Field>
      <CurveEditor key={`${channel.id}:${curveKey}`} curve={curve} onChange={(next) => save(curveKey === null ? { ...asset, curve: next }
        : { ...asset, keyCurves: [...asset.keyCurves.filter((c) => c.key !== curveKey), { key: curveKey, curve: next }] })} />
      {curveKey !== null && <Button size="sm" variant="outline" onClick={() => save({ ...asset, keyCurves: asset.keyCurves.filter((c) => c.key !== curveKey) })}>Use shared curve for this key</Button>}
    </FieldGroup>}
  </div>
}
