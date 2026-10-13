import { ParamControl, ParamGroup, ParamRow, type EffectParamsOf } from "@/features/params"
import type { EditorProps } from "../editor-props"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"
export function VocoderEditor({ params, bind }: EditorProps<EffectParamsOf<"vocoder">>) {
  const count = Math.round(bind.value("bandCount"))
  return <div className="flex flex-col gap-3" data-slot="vocoder-editor">
    <ParamGroup title="Inputs">
      <p className="text-xs text-muted-foreground">{params.synthCarrier ? (params.keyIsCarrier ? t("Built-in carrier. Main input: modulator.") : t("Built-in carrier. Mixer sidechain key: modulator.")) : (params.keyIsCarrier ? t("Main input: modulator. Mixer sidechain key: carrier.") : t("Main input: carrier. Mixer sidechain key: modulator."))}{" "}{t("Route an external carrier or modulator here as a mixer sidechain. A missing external input is silent.")}</p>
      <ParamRow columns={3}><ParamControl {...bind("keyIsCarrier")} /><ParamControl {...bind("synthCarrier")} /><ParamControl {...bind("carrierHz")} disabled={!params.synthCarrier} /></ParamRow>
    </ParamGroup>
    <ParamGroup title="Band bank">
      <svg viewBox="0 0 320 45" role="img" aria-label={message("{count}-band filter bank, 80 Hz to 12 kHz", { count: plainNumber(count) })} className="h-12 w-full text-primary">
        {Array.from({ length: count }, (_, i) => <path key={i} d={`M ${i * 320 / count} 40 L ${(i + .5) * 320 / count} 8 L ${(i + 1) * 320 / count} 40`} fill="none" stroke="currentColor" />)}
      </svg>
      <p className="text-xs text-muted-foreground">{t("Filter layout. 16 bands: classic resolution. 32 bands: detailed resolution.")}</p>
      <ParamRow columns={2}><ParamControl {...bind("bandCount")} /><ParamControl {...bind("color")} /></ParamRow>
    </ParamGroup>
    <ParamRow columns={4}>{["attackMs", "releaseMs", "gain", "mix"].map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
  </div>
}
