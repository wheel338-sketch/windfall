import { useEffect, useRef, useState } from "react"
import { Button } from "@/components/ui/button"
import { Field, FieldLabel, FieldDescription } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { ParamControl, ParamGroup, ParamRow, type EffectParamsOf } from "@/features/params"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import type { EditorProps } from "../editor-props"
import { readImpulseWav, linearPhaseImpulse } from "./impulse"
export function ConvolverEditor({ effect, track, bind }: EditorProps<EffectParamsOf<"convolver">>) {
  const impulse = useProjectStore((s) => s.project.mixer.tracks.find((t) => t.id === track)?.effects.find((e) => e.id === effect)?.impulse)
  const [error, setError] = useState("")
  const [gains, setGains] = useState([0, 0, 0])
  const request = useRef(0)
  useEffect(() => () => { ++request.current }, [effect, track])
  const apply = async (asset: ReturnType<typeof linearPhaseImpulse> | null) => {
    if (track === undefined) return
    try { await dispatch({ type: "setConvolverImpulse", track, effect, impulse: asset }); setError("") }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)) }
  }
  return <div className="flex flex-col gap-3" data-slot="convolver-editor">
    <Field>
      <FieldLabel htmlFor={`ir-${effect}`}>Impulse response WAV</FieldLabel>
      <Input id={`ir-${effect}`} type="file" accept=".wav,audio/wav" onChange={(event) => {
        const file = event.currentTarget.files?.[0]
        if (!file) return
        const ticket = ++request.current, generation = getProjectGeneration()
        if (file.size > 2 * 1024 * 1024) { setError("IR WAV exceeds the 2 MiB import budget"); return }
        void file.arrayBuffer().then((buffer) => {
          if (ticket !== request.current || generation !== getProjectGeneration()) return
          return apply(readImpulseWav(buffer, file.name))
        }).catch((cause: unknown) => { if (ticket === request.current) setError(cause instanceof Error ? cause.message : String(cause)) })
      }} />
      <FieldDescription>{impulse ? `${impulse.name}: ${impulse.frames.length} stereo frames at ${impulse.sampleRate} Hz` : "No IR loaded: unit impulse"}</FieldDescription>
    </Field>
    <Button variant="outline" onClick={() => { ++request.current; void apply(null) }}>Clear impulse</Button>
    <ParamGroup title="Linear-phase EQ">
      <div className="grid grid-cols-3 gap-2">{["Low (<500 Hz)", "Mid", "High (>2 kHz)"].map((label, index) => <Field key={label}>
        <FieldLabel htmlFor={`eq-${effect}-${index}`}>{label} dB</FieldLabel>
        <Input id={`eq-${effect}-${index}`} type="number" min={-24} max={24} value={gains[index]} onChange={(e) => setGains(gains.map((v, i) => i === index ? Number(e.target.value) : v))} />
      </Field>)}</div>
      <Button variant="outline" onClick={() => { ++request.current; try { void apply(linearPhaseImpulse(gains[0], gains[1], gains[2])) } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)) } }}>Apply EQ impulse</Button>
      <p className="text-xs text-muted-foreground">Symmetric FIR adds 512 frames of group delay at 48 kHz, plus the 128-frame convolution buffer.</p>
    </ParamGroup>
    <ParamRow columns={2}><ParamControl {...bind("mix")} /><ParamControl {...bind("gain")} /></ParamRow>
    {error && <p role="alert" className="text-xs text-destructive">{error}</p>}
  </div>
}
