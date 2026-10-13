import { useEffect, useRef, useState } from "react"
import type { RackAudioAsset } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { readImpulseWav } from "@/features/effects/spectral/impulse"
import { useProjectStore } from "@/lib/store"
import { getProjectGeneration } from "@/lib/store/replaced"

export function RackAudioInput({ label, audio, apply }: { label: string; audio: RackAudioAsset | null; apply(audio: RackAudioAsset | null): Promise<unknown> }) {
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const request = useRef(0)
  useEffect(() => () => { ++request.current }, [])
  async function load(file: File) {
    const ticket = ++request.current, generation = getProjectGeneration(), revision = useProjectStore.getState().revision
    setBusy(true); setError(null)
    try {
      if (file.size > 64 * 1024 * 1024) throw new Error("Rack audio exceeds the 64 MiB import limit")
      const data = await file.arrayBuffer()
      if (ticket !== request.current || generation !== getProjectGeneration()) return
      if (revision !== useProjectStore.getState().revision) throw new Error("The project changed during audio loading; choose the source again")
      const decoded = readImpulseWav(data, file.name, 64 * 1024 * 1024 / 8)
      if (!await apply({ name: decoded.name, sampleRate: decoded.sampleRate, frames: decoded.frames })) throw new Error("Rack source change was refused; the previous asset was retained")
    } catch (cause) { if (ticket === request.current && generation === getProjectGeneration()) setError(cause instanceof Error ? cause.message : String(cause)) }
    finally { if (ticket === request.current && generation === getProjectGeneration()) setBusy(false) }
  }
  return <Field>
    <FieldLabel htmlFor={`rack-audio-${label}`}>{audio?.frames === null ? `Relink ${label}` : `Load ${label} WAV`}</FieldLabel>
    <Input id={`rack-audio-${label}`} type="file" accept=".wav" disabled={busy} onChange={(event) => { const file = event.target.files?.[0]; event.target.value = ""; if (file) void load(file) }} />
    <FieldDescription>{audio ? audio.frames ? `${audio.name}: ${audio.frames.length} frames at ${audio.sampleRate} Hz. Audio is carried inside rack presets.` : `Missing source: ${audio.name}. Choose a replacement WAV to relink.` : "No owned audio source"}</FieldDescription>
    <Button variant="outline" disabled={busy || !audio} onClick={() => { ++request.current; setBusy(false); void apply(null) }}>Clear {label}</Button>
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </Field>
}
