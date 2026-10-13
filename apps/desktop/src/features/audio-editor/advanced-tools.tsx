import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Amount } from "./tools-panel"
import type { AudioEditOperation, AudioEditPreview } from "./types"
import { validSelection, type Selection } from "./selection"
import { translatedText as t } from "@/lib/i18n"

export function AdvancedTools({ preview, selection, disabled, onApply }: {
  preview: AudioEditPreview; selection: Selection; disabled: boolean
  onApply(operation: AudioEditOperation): void
}) {
  const [low, setLow] = useState(0)
  const [mid, setMid] = useState(-3)
  const [high, setHigh] = useState(0)
  const [blur, setBlur] = useState(25)
  const [wet, setWet] = useState(50)
  const [impulse, setImpulse] = useState<Selection | null>(null)
  const [threshold, setThreshold] = useState(0.98)
  const [silenceDb, setSilenceDb] = useState(-60)
  const [silenceMs, setSilenceMs] = useState(50)
  const [crossfadeMs, setCrossfadeMs] = useState(10)
  const within = (value: number, min: number, max: number) => Number.isFinite(value) && value >= min && value <= max
  const amount = (id: string, label: string, value: number, onChange: (value: number) => void, min: number, max: number, step = 1) =>
    <Amount id={id} label={label} value={value} onChange={onChange} range={{min,max}} step={step} disabled={disabled} />
  const action = (label: string, operation: AudioEditOperation, valid = true) =>
    <Button size="sm" variant="outline" disabled={disabled || !valid} onClick={() => onApply(operation)}>{label}</Button>
  const impulseValid = impulse !== null && validSelection(impulse, preview.frames) && impulse.end - impulse.start <= 32768
  return <div role="group" aria-label={t("Advanced processing")} className="flex flex-col gap-3 rounded-md border p-2">
    <p className="text-sm font-medium">{t("Advanced processing")}</p>
    <div className="flex flex-wrap items-end gap-2">
      {amount("editor-eq-low", t("Low EQ (dB)"), low, setLow, -24, 24, 0.5)}
      {amount("editor-eq-mid", t("Mid EQ (dB)"), mid, setMid, -24, 24, 0.5)}
      {amount("editor-eq-high", t("High EQ (dB)"), high, setHigh, -24, 24, 0.5)}
      {action(t("Equalize selection"), {equalize:{lowDb:low,midDb:mid,highDb:high}}, [low,mid,high].every(v=>within(v,-24,24)) && [low,mid,high].some(v=>v!==0))}
    </div>
    <p className="text-xs text-muted-foreground">{t("Smooth bands at 250 Hz and 4 kHz. Boosts that clip are refused.")}</p>
    <div className="flex flex-wrap items-end gap-2">
      {amount("editor-blur", t("Spectral blur (%)"), blur, setBlur, 1, 100)}
      {action(t("Blur selection"), {blur:{amount:blur/100}}, within(blur,1,100))}
      {amount("editor-wet", t("Convolution mix (%)"), wet, setWet, 1, 100)}
      <Button size="sm" variant="outline" disabled={disabled || selection.end-selection.start > 32768} onClick={() => setImpulse({...selection})}>{t("Use selection as impulse")}</Button>
      {action(t("Convolve selection"), {convolve:{impulse:{startFrame:impulse?.start ?? 0,endFrame:impulse?.end ?? 0},wet:wet/100}}, impulseValid && within(wet,1,100))}
    </div>
    <p className="text-xs text-muted-foreground">{t("Capture an impulse of up to 32768 frames, then select audio to convolve. The tail is cropped at the selection end.")}</p>
    {impulse && <output aria-label={t("Captured impulse")}>{t("Impulse: frames")}{" "}{impulse.start}–{impulse.end}</output>}
    <div className="flex flex-wrap items-end gap-2">
      {amount("editor-clipping", t("Clipping threshold"), threshold, setThreshold, 0.5, 1, 0.01)}
      {action(t("Repair clipping"), {repairClipping:{threshold}}, within(threshold,0.5,1))}
      {amount("editor-loop", t("Loop crossfade (ms)"), crossfadeMs, setCrossfadeMs, 0.1, 60000, 0.1)}
      {action(t("Tune loop seam"), {loopTune:{crossfadeFrames:Math.round(crossfadeMs*preview.sampleRate/1000)}}, within(crossfadeMs,0.1,60000) && Math.round(crossfadeMs*preview.sampleRate/1000) <= (selection.end-selection.start)/2)}
    </div>
    <div className="flex flex-wrap items-end gap-2">
      {amount("editor-strip-db", t("Split below (dBFS)"), silenceDb, setSilenceDb, -120, -6)}
      {amount("editor-strip-ms", t("Minimum silence (ms)"), silenceMs, setSilenceMs, 1, 120000)}
      {action(t("Split and strip silence"), {stripSilence:{thresholdDb:silenceDb,minSilenceFrames:Math.round(silenceMs*preview.sampleRate/1000)}}, within(silenceDb,-120,-6) && within(silenceMs,1,120000))}
    </div>
    <p className="text-xs text-muted-foreground">{t("Separate clips keep their original song positions. Each operation is one undo step.")}</p>
  </div>
}
