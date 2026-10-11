import { ParamControl, ParamGroup, ParamRow, type EffectParamsOf } from "@/features/params"
import type { EditorProps } from "../editor-props"
import { CIRCUIT_LAYOUT } from "./motion"

export function LushSpaceEditor({ bind }: EditorProps<EffectParamsOf<"lushSpace">>) {
  return (
    <div data-slot="lush-space-editor" className={CIRCUIT_LAYOUT}>
      <ParamGroup title="Space">
        <ParamRow columns={3}>
          <ParamControl {...bind("size")} description="Changes reflection spacing separately from the nominal decay" />
          <ParamControl {...bind("decayS")} />
          <ParamControl {...bind("preDelayMs")} />
          <ParamControl {...bind("damping")} />
          <ParamControl {...bind("earlyLevel")} />
          <ParamControl {...bind("width")} />
        </ParamRow>
      </ParamGroup>
      <ParamGroup title="Motion and pitch">
        <ParamRow columns={2}>
          <ParamControl {...bind("modulationRateHz")} />
          <ParamControl {...bind("modulationDepthMs")} />
          <ParamControl {...bind("pitchSemitones")} description="Pitch of the feed-forward wet voice, from one octave down to one octave up" />
          <ParamControl {...bind("pitchAmount")} description="Crossfade from the plain wet space to its pitched voice; zero keeps the original hall" />
        </ParamRow>
        <ParamControl {...bind("mix")} layout="inline" />
      </ParamGroup>
    </div>
  )
}
