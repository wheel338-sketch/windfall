import { ParamControl, ParamGroup, ParamRow, type EffectParamsOf } from "@/features/params"
import type { EditorProps } from "../editor-props"
import { CIRCUIT_LAYOUT, Motion } from "./motion"
import { translatedText as t } from "@/lib/i18n"

const MODES = [
  () => t("Four adjustable detuned voices per side."),
  () => t("Two clocked bucket lines: slow opposed triangle sweeps."),
  () => t("Two clocked bucket lines: faster opposed triangle sweeps."),
  () => t("Fast shallow bucket vibrato; both sides move together."),
]

export function BucketChorusEditor({ bind }: EditorProps<EffectParamsOf<"vintageChorus">>) {
  const mode = bind.value("mode")
  const fixed = mode > 0
  return (
    <div data-slot="bucket-chorus-editor" className={CIRCUIT_LAYOUT}>
      <ParamGroup title="Circuit">
        <ParamControl {...bind("mode")} layout="inline" />
        <p className="text-[10px] text-muted-foreground" data-slot="bucket-mode">{MODES[mode]?.()}</p>
        <ParamRow columns={2}>
          <ParamControl {...bind("toneHz")} description="Wet tone; bucket modes also use a two-pole input filter" />
          <ParamControl {...bind("mix")} />
        </ParamRow>
      </ParamGroup>
      <ParamGroup title="Free voices">
        <ParamRow columns={3}>
          {(["delayMs", "depthMs", "spread"] as const).map((id) => (
            <ParamControl key={id} {...bind(id)} disabled={fixed || bind.disabled} />
          ))}
        </ParamRow>
        <Motion bind={bind} disabled={fixed} />
      </ParamGroup>
    </div>
  )
}
