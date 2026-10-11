import { ParamControl, ParamGroup, ParamRow, type EffectParamsOf } from "@/features/params"
import type { EditorProps } from "../editor-props"
import { CIRCUIT_LAYOUT, Motion } from "./motion"

export function StackedFlangerEditor({ bind }: EditorProps<EffectParamsOf<"stackedFlanger">>) {
  const order = bind.value("order")
  return (
    <div data-slot="stacked-flanger-editor" className={CIRCUIT_LAYOUT}>
      <ParamGroup title="Comb stack">
        <ParamRow columns={3}>
          <ParamControl {...bind("order")} description="One to four swept combs in series; each adds its own notches" />
          <ParamControl {...bind("delayMs")} />
          <ParamControl {...bind("depthMs")} />
        </ParamRow>
        <p data-slot="comb-chain" className="font-readout text-[10px] text-muted-foreground">
          {Array.from({ length: order }, (_, i) => `Comb ${i + 1}`).join(" → ")}
        </p>
        <Motion bind={bind} rate="rate1Hz" />
        <ParamControl {...bind("rate2Hz")} disabled={bind.value("sync") >= 0.5 || bind.disabled}
          description="Second rate; tempo sync fixes its ratio to the first at 1.382" />
      </ParamGroup>
      <ParamGroup title="Feedback and stereo">
        <ParamRow columns={4}>
          <ParamControl {...bind("feedback")} />
          <ParamControl {...bind("toneHz")} />
          <ParamControl {...bind("spread")} />
          <ParamControl {...bind("mix")} />
        </ParamRow>
      </ParamGroup>
    </div>
  )
}
