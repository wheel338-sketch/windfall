import type { EffectParams, OverdriveParams, WaveshaperParams } from "@/bindings"
import type { CurvePoint } from "@/bindings/CurvePoint"
import { ParamControl, ParamRow } from "@/features/params"
import { dispatch, useProjectStore } from "@/lib/store"

import type { EditorProps } from "../editor-props"
import { CurveEditor } from "./curve-editor"

/** Read the current slot at commit, preserving scalar edits made during a drawing. */
export function commitAsset(effect: number, track: number | undefined, kind: EffectParams["type"], field: string, value: unknown) {
  if (track === undefined) return
  const slot = useProjectStore.getState().project.mixer.tracks.find((row) => row.id === track)?.effects.find((row) => row.id === effect)
  if (!slot || slot.params.type !== kind) return
  void dispatch({ type: "setEffectParams", track, effect, params: { ...slot.params, [field]: value } })
}

export function OverdriveEditor({ bind }: EditorProps<OverdriveParams>) {
  return <div role="group" aria-label="Overdrive settings" className="flex flex-col gap-2">
    <ParamRow columns={4}>{["driveDb", "asymmetry", "emphasis", "preBandHz", "toneHz", "outputDb", "hot", "oversampling"].map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
  </div>
}

export function WaveshaperEditor({ effect, track, params, bind }: EditorProps<WaveshaperParams>) {
  const fallback: CurvePoint[] = params.curve.map((y, i) => ({ x: i / 32, y: (y + 1) / 2, tension: 0 }))
  return <div role="group" aria-label="Waveshaper settings" className="flex flex-col gap-2">
    <CurveEditor key={effect} label="Transfer curve" points={params.shape} fallback={fallback} onCommit={(shape) => commitAsset(effect, track, "waveshaper", "shape", shape)} />
    <ParamRow columns={5}>{["inputDb", "outputDb", "oversampling", "mix", "bipolar"].map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
  </div>
}
