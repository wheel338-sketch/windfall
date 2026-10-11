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

import { BandSplitEditor } from "./band-split/band-split-editor"
import { CompressorEditor } from "./compressor/compressor-editor"
import { DelayEditor } from "./delay/delay-editor"
import {
  BassHarmonicsEditor,
  MasterLimiterEditor,
  OneKnobEditor,
  StageStackEditor,
  TransientShaperEditor,
} from "./dynamics/dynamics-editors"
import {
  MultibandCompressorEditor,
  MultibandMaximizerEditor,
} from "./dynamics/multiband-editor"
import type { EditorProps } from "./editor-props"
import { EqEditor } from "./eq/eq-editor"
import { FilterBankEditor } from "./filter-bank/filter-bank-editor"
import { FilterEditor } from "./filter/filter-editor"
import { MorphEqEditor } from "./graphic/morph-eq-editor"
import { SevenBandEditor } from "./graphic/seven-band-editor"
import { LimiterEditor } from "./limiter/limiter-editor"
import { PanMotionEditor } from "./motion/pan-motion-editor"
import { ReverbEditor } from "./reverb/reverb-editor"
<<<<<<< HEAD
import { SpreaderEditor } from "./stereo/spreader-editor"
import { StereoEnhancerEditor } from "./stereo/stereo-enhancer-editor"
import { EchoBankEditor } from "./echo-bank/echo-bank-editor"
import { FrequencyDelayEditor } from "./frequency-delay/frequency-delay-editor"
=======
import { EnvelopeSourceEditor, KeyboardSourceEditor, FormulaSourceEditor, PeakSourceEditor, XySourceEditor, XyzSourceEditor, SurfaceSourceEditor, VoltageSourceEditor } from "@/features/internal-controllers/editors"
>>>>>>> gpt/w3-control-sources
import { TunerEditor } from "./tuner/tuner-editor"
import { RoomEditor } from "./circuits/room-editor"
import { LushSpaceEditor } from "./circuits/lush-space-editor"
import { BucketChorusEditor } from "./circuits/bucket-chorus-editor"
import { StagePhaserEditor } from "./circuits/stage-phaser-editor"
import { SwarmChorusEditor } from "./circuits/swarm-chorus-editor"
import { StackedFlangerEditor } from "./circuits/stacked-flanger-editor"
import { DriveChainEditor, GuitarRackEditor, OverdriveEditor, WaveshaperEditor } from "./nonlinear/editors"
import { PerformanceRackEditor, ScratchEditor, TimeTransportEditor, VolumeGateEditor } from "./nonlinear/performance-editors"

import { ConvolverEditor } from "./spectral/convolver-editor"
import { PitchShiftEditor, PitchCorrectEditor } from "./spectral/pitch-editor"
import { VocoderEditor } from "./spectral/vocoder-editor"

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
  multibandCompressor: MultibandCompressorEditor,
  multibandMaximizer: MultibandMaximizerEditor,
  oneKnob: OneKnobEditor,
  transientShaper: TransientShaperEditor,
  bassHarmonics: BassHarmonicsEditor,
  stageStack: StageStackEditor,
  masterLimiter: MasterLimiterEditor,
  sevenBand: SevenBandEditor,
  spreader: SpreaderEditor,
  stereoEnhancer: StereoEnhancerEditor,
  panLfo: PanMotionEditor,
  morphEq: MorphEqEditor,
  tuner: TunerEditor,
<<<<<<< HEAD
  bandSplit: BandSplitEditor,
  selectableFilter: FilterEditor,
  filterBank: FilterBankEditor,
  echoBank: EchoBankEditor,
  frequencyDelay: FrequencyDelayEditor,
  room: RoomEditor,
  lushSpace: LushSpaceEditor,
  vintageChorus: BucketChorusEditor,
  vintagePhaser: StagePhaserEditor,
  hyperChorus: SwarmChorusEditor,
  stackedFlanger: StackedFlangerEditor,
  overdrive: OverdriveEditor,
  waveshaper: WaveshaperEditor,
  guitarRack: GuitarRackEditor,
  driveChain: DriveChainEditor,
  volumeGate: VolumeGateEditor,
  timeTransport: TimeTransportEditor,
  performanceRack: PerformanceRackEditor,
  scratch: ScratchEditor,
  convolver: ConvolverEditor,
  vocoder: VocoderEditor,
  pitchShift: PitchShiftEditor,
  pitchCorrect: PitchCorrectEditor,
=======
  noteEnvelope: EnvelopeSourceEditor,
  keyboardSource: KeyboardSourceEditor,
  formulaSource: FormulaSourceEditor,
  envelopeFollower: PeakSourceEditor,
  xyPad: XySourceEditor,
  xyzPad: XyzSourceEditor,
  controlSurface: SurfaceSourceEditor,
  voltageSource: VoltageSourceEditor,
>>>>>>> gpt/w3-control-sources
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
      key={slot.id}
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
