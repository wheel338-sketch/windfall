import { useCallback, type ComponentType } from "react"

import type { EffectKind, EffectParams, EffectSlot, TrackId } from "@/bindings"
import {
  effectDescriptor,
  GenericParamEditor,
  useParamBinding,
  type EffectParamsOf,
  type ParamDescriptor,
  type SetParam,
} from "@/features/params"
import { useParamAutomation } from "@/features/automation/live"
import { dispatch } from "@/lib/store"

import { CompressorEditor } from "./compressor/compressor-editor"
import { DelayEditor } from "./delay/delay-editor"
import type { EditorProps } from "./editor-props"
import { EqEditor } from "./eq/eq-editor"
import { LimiterEditor } from "./limiter/limiter-editor"
import { ReverbEditor } from "./reverb/reverb-editor"
import { TunerEditor } from "./tuner/tuner-editor"
import { GuitarRackEditor, OverdriveEditor, WaveshaperEditor } from "./nonlinear/editors"

type Editors = {
  [Kind in EffectKind]?: ComponentType<EditorProps<EffectParamsOf<Kind>>>
}

/**
 * The purpose-built editors. A kind that is not listed here gets the
 * generic editor, which is made from its descriptor alone, so an effect
 * added to the core is usable before anyone has designed a panel for it.
 */
const EDITORS: Editors = {
  eq: EqEditor,
  compressor: CompressorEditor,
  limiter: LimiterEditor,
  reverb: ReverbEditor,
  delay: DelayEditor,
  tuner: TunerEditor,
  overdrive: OverdriveEditor,
  waveshaper: WaveshaperEditor,
  guitarRack: GuitarRackEditor,
}

type AnyEditor = ComponentType<EditorProps<EffectParams>>

/** The purpose-built editor of a kind, if it has one. */
export function customEditor(kind: string): AnyEditor | undefined {
  // Each editor takes the settings of its own kind, and `kind` is the tag
  // those settings carry, so the two always belong together.
  return (EDITORS as Partial<Record<string, AnyEditor>>)[kind]
}

export type EffectEditorProps = {
  /** The track the effect is on, which is how commands reach it. */
  trackId: TrackId
  slot: EffectSlot
  /** Looks up the settings table of a kind. Tests hand in their own. */
  describe?: (kind: EffectKind) => ParamDescriptor
}

function useSetParam(trackId: TrackId, slot: EffectSlot): SetParam {
  const effect = slot.id
  return useCallback(
    (param, value, gesture) =>
      dispatch(
        { type: "setEffectParam", track: trackId, effect, param, value },
        gesture
      ),
    [trackId, effect]
  )
}

type BoundEditorProps = EffectEditorProps & {
  Editor: AnyEditor
  descriptor: ParamDescriptor
}

/** Makes every setting of an effect automatable from its own control. */
function useEffectAutomation(trackId: TrackId, slot: EffectSlot) {
  const effect = slot.id
  return useParamAutomation(
    (param) => ({ type: "effectParam", track: trackId, effect, param }),
    `${trackId}:${effect}`
  )
}

function BoundEditor({ trackId, slot, Editor, descriptor }: BoundEditorProps) {
  const setParam = useSetParam(trackId, slot)
  const automation = useEffectAutomation(trackId, slot)
  const bind = useParamBinding({
    descriptor,
    params: slot.params,
    setParam,
    ...automation,
  })
  return (
    <Editor
      effect={slot.id}
      track={trackId}
      params={slot.params}
      bind={bind}
    />
  )
}

function FallbackEditor({
  trackId,
  slot,
  descriptor,
}: EffectEditorProps & { descriptor: ParamDescriptor }) {
  const setParam = useSetParam(trackId, slot)
  const automation = useEffectAutomation(trackId, slot)
  return (
    <GenericParamEditor
      descriptor={descriptor}
      params={slot.params}
      setParam={setParam}
      {...automation}
    />
  )
}

/**
 * The editor of one effect. Every control in it changes one setting through
 * `setEffectParam`, and every change of one drag shares a gesture id, so a
 * drag is one undo step.
 */
export function EffectEditor({
  trackId,
  slot,
  describe = effectDescriptor,
}: EffectEditorProps) {
  const kind = slot.params.type
  const descriptor = describe(kind)
  const Editor = customEditor(kind)
  return Editor ? (
    <BoundEditor
      trackId={trackId}
      slot={slot}
      Editor={Editor}
      descriptor={descriptor}
    />
  ) : (
    <FallbackEditor trackId={trackId} slot={slot} descriptor={descriptor} />
  )
}
