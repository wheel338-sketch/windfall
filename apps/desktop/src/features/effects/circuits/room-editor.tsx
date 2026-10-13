import { ParamControl, ParamGroup, ParamRow, type EffectParamsOf } from "@/features/params"
import type { EditorProps } from "../editor-props"
import { CIRCUIT_LAYOUT } from "./motion"
import { translatedText as t } from "@/lib/i18n"

export function RoomEditor({ bind }: EditorProps<EffectParamsOf<"room">>) {
  const first = bind.value("preDelayMs") + 3.7 * (0.6 + bind.value("size"))
  return (
    <div data-slot="room-editor" className={CIRCUIT_LAYOUT}>
      <ParamGroup title="Room reflections">
        <ParamRow columns={3}>
          <ParamControl {...bind("size")} />
          <ParamControl {...bind("preDelayMs")} />
          <ParamControl {...bind("earlyLevel")} />
        </ParamRow>
        <p data-slot="room-first-reflection" className="text-[10px] text-muted-foreground">{t("First left reflection: about")}{" "}{first.toFixed(1)}{" "}{t("ms. Four early taps lead into four damped combs.")}</p>
      </ParamGroup>
      <ParamGroup title="Decay and output">
        <ParamRow columns={4}>
          <ParamControl {...bind("decayS")} description="Nominal low-frequency time to decay by 60 dB" />
          <ParamControl {...bind("damping")} />
          <ParamControl {...bind("width")} />
          <ParamControl {...bind("mix")} />
        </ParamRow>
      </ParamGroup>
    </div>
  )
}
