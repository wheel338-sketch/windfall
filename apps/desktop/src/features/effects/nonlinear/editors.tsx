import type { DriveChainParams, EffectParams, GuitarRackParams, OverdriveParams, WaveshaperParams } from "@/bindings"
import { Button } from "@/components/ui/button"
import type { CurvePoint } from "@/bindings/CurvePoint"
import { ParamControl, ParamRow } from "@/features/params"
import { dispatch, useProjectStore } from "@/lib/store"

import type { EditorProps } from "../editor-props"
import { CurveEditor } from "./curve-editor"
import { useEffectAssetCommit } from "../asset-owner"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

/** Read the current slot at commit, preserving scalar edits made during a drawing. */
export function commitAsset(effect: number, track: number | undefined, kind: EffectParams["type"], field: string, value: unknown) {
  if (track === undefined) return
  const slot = useProjectStore.getState().project.mixer.tracks.find((row) => row.id === track)?.effects.find((row) => row.id === effect)
  if (!slot || slot.params.type !== kind) return
  void dispatch({ type: "setEffectParams", track, effect, params: { ...slot.params, [field]: value } })
}

export function OverdriveEditor({ bind }: EditorProps<OverdriveParams>) {
  return <div role="group" aria-label={t("Overdrive settings")} className="flex flex-col gap-2">
    <ParamRow columns={4}>{["driveDb", "asymmetry", "emphasis", "preBandHz", "toneHz", "outputDb", "hot", "oversampling"].map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
  </div>
}

export function WaveshaperEditor({ effect, track, params, bind }: EditorProps<WaveshaperParams>) {
  const commit = useEffectAssetCommit(effect, track, "waveshaper", commitAsset)
  const fallback: CurvePoint[] = params.curve.map((y, i) => ({ x: i / 32, y: (y + 1) / 2, tension: 0 }))
  return <div role="group" aria-label={t("Waveshaper settings")} className="flex flex-col gap-2">
    <CurveEditor key={effect} label={t("Transfer curve")} points={params.shape} fallback={fallback} onCommit={(shape) => commit("shape", shape)} />
    {params.shape.length > 0 ? <Button size="sm" variant="outline" onClick={() => commit("shape", [])}>{t("Use automatable knots")}</Button> : <ParamRow columns={6}>{params.curve.map((_, index) => <ParamControl key={index} {...bind(`curve.${index}`)} />)}</ParamRow>}
    <ParamRow columns={5}>{["inputDb", "outputDb", "oversampling", "mix", "bipolar"].map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
  </div>
}

const PEDALS = ["Boost", "Overdrive", "Fuzz", "Octave", "Treble", "Scoop", "Wah", "Compressor", "Gate", "Spring", "Cabinet"]
export function GuitarRackEditor({ bind }: EditorProps<GuitarRackParams>) {
  return <div role="group" aria-label={t("Guitar Rack settings")} className="flex flex-col gap-2">
    <ParamRow columns={5}>{["model", "driveDb", "outputDb", "cabinet", "oversampling"].map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
    <p className="text-xs text-muted-foreground">{t("The legacy Pedal selection also enables that pedal. Choose Clean boost to use only the switches below. Signal: gate → compressor → wah → treble → boost → drive → fuzz → octave → scoop → spring → cabinet.")}</p>
    <div className="grid grid-cols-[repeat(auto-fit,minmax(12rem,1fr))] gap-2">
      {PEDALS.map((name, index) => <div key={name} role="group" aria-label={message("{name} pedal", { name })} className="flex flex-col gap-2 rounded border border-border p-2">
        <ParamControl {...bind(`pedals.${index}.on`)} label={t(name)} />
        <ParamRow columns={2}><ParamControl {...bind(`pedals.${index}.amount`)} /><ParamControl {...bind(`pedals.${index}.tone`)} /></ParamRow>
      </div>)}
    </div>
  </div>
}

export function DriveChainEditor({ effect, track, params, bind }: EditorProps<DriveChainParams>) {
  return <div role="group" aria-label={t("Drive Chain settings")} className="flex flex-col gap-2">
    <ParamRow columns={2}><ParamControl {...bind("outputDb")} /><ParamControl {...bind("oversampling")} /></ParamRow>
    {params.stages.map((stage, index) => <div key={index} role="group" aria-label={message("Module {index}", { index: plainNumber(index + 1) })} className="flex flex-col gap-2 rounded border border-border p-2">
      <div className="flex items-center gap-2"><ParamControl {...bind(`stages.${index}.model`)} />
        <Button size="sm" variant="outline" disabled={index === 0} onClick={() => {
          const stages = [...params.stages]
          ;[stages[index - 1], stages[index]] = [stages[index], stages[index - 1]]
          commitAsset(effect, track, "driveChain", "stages", stages)
        }}>{t("Move module")}{" "}{index + 1}{" "}{t("earlier")}</Button>
      </div>
      <ParamRow columns={3}>
        <ParamControl {...bind(`stages.${index}.gainDb`)} />
        {["lowpass", "highpass"].includes(stage.model) && <><ParamControl {...bind(`stages.${index}.cutoffHz`)} /><ParamControl {...bind(`stages.${index}.resonance`)} /></>}
        {stage.model === "chorus" && <><ParamControl {...bind(`stages.${index}.depth`)} /><ParamControl {...bind(`stages.${index}.rateHz`)} /></>}
        {stage.model === "cabinet" && <ParamControl {...bind(`stages.${index}.cabinet`)} />}
      </ParamRow>
    </div>)}
  </div>
}
