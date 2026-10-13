import { ParamControl, ParamGroup, ParamRow, type EffectParamsOf } from "@/features/params"
import type { EditorProps } from "../editor-props"
import { CIRCUIT_LAYOUT, Motion } from "./motion"
import { translatedText as t } from "@/lib/i18n"

const COUNTS = [12, 4, 6, 8]

export function StagePhaserEditor({ bind }: EditorProps<EffectParamsOf<"vintagePhaser">>) {
  const stages = COUNTS[bind.value("stages")]
  return (
    <div data-slot="stage-phaser-editor" className={CIRCUIT_LAYOUT}>
      <ParamGroup title="Allpass circuit">
        <ParamControl {...bind("stages")} layout="inline" />
        <p className="text-[10px] text-muted-foreground" data-slot="phaser-notches">
          {stages / 2}{" "}{t("moving notches. Four stages with zero stagger use the pedal circuit.")}
        </p>
        <ParamRow columns={3}>
          <ParamControl {...bind("stagger")} description="At zero all stages have the same corner frequency" />
          <ParamControl {...bind("feedback")} description="Signed feedback sharpens the phasing" />
          <ParamControl {...bind("colorHz")} />
        </ParamRow>
      </ParamGroup>
      <ParamGroup title="Sweep">
        <ParamRow columns={3}>
          <ParamControl {...bind("minHz")} />
          <ParamControl {...bind("maxHz")} />
          <ParamControl {...bind("stereoPhase")} />
        </ParamRow>
        <Motion bind={bind} />
        <ParamControl {...bind("mix")} layout="inline" />
      </ParamGroup>
    </div>
  )
}
