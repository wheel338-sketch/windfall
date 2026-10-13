import { ParamControl, ParamGroup, ParamRow, type EffectParamsOf } from "@/features/params"
import type { EditorProps } from "../editor-props"
import { CIRCUIT_LAYOUT, Motion } from "./motion"
import { translatedText as t } from "@/lib/i18n"

export function SwarmChorusEditor({ bind }: EditorProps<EffectParamsOf<"hyperChorus">>) {
  const voices = bind.value("voices")
  return (
    <div data-slot="swarm-chorus-editor" className={CIRCUIT_LAYOUT}>
      <ParamGroup title="Voice cloud">
        <ParamRow columns={3}>
          <ParamControl {...bind("voices")} description="Two to sixteen independently detuned voices per side; each count is an actual circuit change" />
          <ParamControl {...bind("delayMs")} />
          <ParamControl {...bind("depthMs")} />
        </ParamRow>
        <p data-slot="swarm-count" className="text-[10px] text-muted-foreground">{voices}{" "}{t("voices per side, with staggered times and rates.")}</p>
        <Motion bind={bind} />
      </ParamGroup>
      <ParamGroup title="Stereo and tone">
        <ParamRow columns={3}>
          <ParamControl {...bind("spread")} description="Offsets right-side phases and crossfeeds alternating voices" />
          <ParamControl {...bind("toneHz")} />
          <ParamControl {...bind("mix")} />
        </ParamRow>
      </ParamGroup>
    </div>
  )
}
