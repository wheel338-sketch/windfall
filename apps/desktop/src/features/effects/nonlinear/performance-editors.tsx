import type { PerformanceRackParams, ScratchParams, TimeTransportParams, VolumeGateParams } from "@/bindings"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { ParamControl, ParamRow } from "@/features/params"
import { useProjectStore } from "@/lib/store"
import { useSampleInfo } from "@/features/channel-rack/inspector/sample-info"
import type { EditorProps } from "../editor-props"
import { CurveEditor } from "./curve-editor"
import { commitAsset } from "./editors"

export function VolumeGateEditor({ effect, track, params, bind }: EditorProps<VolumeGateParams>) {
  return <div role="group" aria-label="Volume Gate settings" className="flex flex-col gap-2">
    <CurveEditor key={`time:${effect}`} label="Replay time" points={params.timeCurve} onCommit={(curve) => commitAsset(effect, track, "volumeGate", "timeCurve", curve)} />
    <CurveEditor key={`volume:${effect}`} label="Replay volume" points={params.volumeCurve} fallback={[{ x: 0, y: 1, tension: 0 }, { x: 1, y: 1, tension: 0 }]} onCommit={(curve) => commitAsset(effect, track, "volumeGate", "volumeCurve", curve)} />
    <ParamRow columns={5}>{["loopBeats", "retriggerChance", "retriggerSteps", "feedback", "mix"].map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
  </div>
}

export function TimeTransportEditor({ bind }: EditorProps<TimeTransportParams>) {
  return <div role="group" aria-label="Time Transport settings" className="flex flex-col gap-2">
    <ParamRow columns={3}>{["autoTrigger", "thresholdDb", "sensitivity", "refractoryMs", "trigger", "mode", "loopBeats", "rate", "mix"].map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
    <p className="text-xs text-muted-foreground">Transient capture records from each detected onset for one Slice, then repeats it until another onset. Trigger can also start a capture. Hold and Reverse use the same record.</p>
  </div>
}

export function XyPad({ x, y, onStart, onChange, onEnd }: {
  x: number; y: number; onStart(): void; onChange(x: number, y: number): void; onEnd(): void
}) {
  return <svg role="application" aria-label="Performance XY pad" tabIndex={0} viewBox="0 0 100 100" className="size-56 touch-none rounded border border-border bg-muted text-primary"
    onPointerDown={(event) => {
      if (event.button !== 0) return
      event.currentTarget.setPointerCapture(event.pointerId)
      onStart()
      const r = event.currentTarget.getBoundingClientRect()
      onChange(Math.max(0,Math.min(1,(event.clientX-r.left)/Math.max(1,r.width))),Math.max(0,Math.min(1,1-(event.clientY-r.top)/Math.max(1,r.height))))
    }}
    onPointerMove={(event) => {
      if (!event.currentTarget.hasPointerCapture(event.pointerId)) return
      const r = event.currentTarget.getBoundingClientRect()
      onChange(Math.max(0,Math.min(1,(event.clientX-r.left)/Math.max(1,r.width))),Math.max(0,Math.min(1,1-(event.clientY-r.top)/Math.max(1,r.height))))
    }}
    onPointerUp={(event) => { if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId); onEnd() }}
    onLostPointerCapture={onEnd} onPointerCancel={onEnd} onBlur={onEnd}
    onKeyDown={(event) => {
      if (!["ArrowLeft","ArrowRight","ArrowUp","ArrowDown"].includes(event.key)) return
      event.preventDefault(); onStart()
      onChange(Math.max(0,Math.min(1,x+(event.key === "ArrowRight" ? 0.01 : event.key === "ArrowLeft" ? -0.01 : 0))),Math.max(0,Math.min(1,y+(event.key === "ArrowUp" ? 0.01 : event.key === "ArrowDown" ? -0.01 : 0))))
      onEnd()
    }}
  ><path d="M 0 50 H 100 M 50 0 V 100" stroke="currentColor" opacity="0.2" /><circle cx={x*100} cy={(1-y)*100} r="3" fill="currentColor" /></svg>
}

export function PerformanceRackEditor({ bind }: EditorProps<PerformanceRackParams>) {
  const group = bind.group("performance-xy")
  return <div role="group" aria-label="Performance Rack settings" className="flex flex-col gap-2">
    <XyPad x={bind.value("x")} y={bind.value("y")} onStart={() => { group.onGestureStart(); group.set("xyEnabled",1) }} onChange={(x,y) => { group.set("x",x); group.set("y",y) }} onEnd={group.onGestureEnd} />
    <ParamRow columns={3}>{["model", "loopBeats", "amount", "pitchSemitones", "feedback", "mix", "xyEnabled", "x", "y"].map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
    <p className="text-xs text-muted-foreground">X sets depth; Y sets slice offset, speed, repeat length, filter range, drive bias, orbit rate, fade shape, gate duty or pitch according to the selected treatment.</p>
  </div>
}

export function ScratchEditor({ effect, track, params, bind }: EditorProps<ScratchParams>) {
  const samples = useProjectStore((state) => state.project.samples)
  const asset = samples.find((sample) => sample.id === params.sourceId)
  const info = useSampleInfo(asset)
  return <div role="group" aria-label="Scratch settings" className="flex flex-col gap-2">
    <Field><FieldLabel>Turntable source</FieldLabel>
      <Select value={String(params.sourceId)} items={[{ value: "0", label: "Live record" }, ...samples.map((sample) => ({ value: String(sample.id), label: sample.name }))]} onValueChange={(value) => commitAsset(effect,track,"scratch","sourceId",Number(value))}>
        <SelectTrigger aria-label="Scratch source"><SelectValue /></SelectTrigger><SelectContent><SelectGroup>
          <SelectItem value="0">Live record</SelectItem>{samples.map((sample) => <SelectItem key={sample.id} value={String(sample.id)}>{sample.name}</SelectItem>)}
        </SelectGroup></SelectContent>
      </Select>
      <FieldDescription>Choose loaded project audio. Add an audio file to the channel rack or playlist to make it available here. Position moves the platter; Follow scratch curve performs the saved movement every Record cycle.</FieldDescription>
    </Field>
    {info?.status === "error" && <Alert variant="destructive"><AlertDescription>{info.message}</AlertDescription></Alert>}
    <CurveEditor key={effect} label="Scratch position" points={params.positionCurve} onCommit={(curve) => commitAsset(effect,track,"scratch","positionCurve",curve)} />
    <ParamRow columns={5}>{["position", "freeze", "loopBeats", "mix", "curveEnabled"].map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
  </div>
}
