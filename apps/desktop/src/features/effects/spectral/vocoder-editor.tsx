import { ParamControl, ParamGroup, ParamRow, type EffectParamsOf } from "@/features/params"
import type { EditorProps } from "../editor-props"
export function VocoderEditor({ params, bind }: EditorProps<EffectParamsOf<"vocoder">>) {
  const count = Math.round(bind.value("bandCount"))
  return <div className="flex flex-col gap-3" data-slot="vocoder-editor">
    <ParamGroup title="Inputs">
      <p className="text-xs text-muted-foreground">{params.synthCarrier ? (params.keyIsCarrier ? "Built-in carrier. Main input: modulator." : "Built-in carrier. Mixer sidechain key: modulator.") : (params.keyIsCarrier ? "Main input: modulator. Mixer sidechain key: carrier." : "Main input: carrier. Mixer sidechain key: modulator.")} Route an external carrier or modulator here as a mixer sidechain. A missing external input is silent.</p>
      <ParamRow columns={3}><ParamControl {...bind("keyIsCarrier")} /><ParamControl {...bind("synthCarrier")} /><ParamControl {...bind("carrierHz")} disabled={!params.synthCarrier} /></ParamRow>
    </ParamGroup>
    <ParamGroup title="Band bank">
      <svg viewBox="0 0 320 45" role="img" aria-label={`${count}-band filter bank, 80 Hz to 12 kHz`} className="h-12 w-full text-primary">
        {Array.from({ length: count }, (_, i) => <path key={i} d={`M ${i * 320 / count} 40 L ${(i + .5) * 320 / count} 8 L ${(i + 1) * 320 / count} 40`} fill="none" stroke="currentColor" />)}
      </svg>
      <p className="text-xs text-muted-foreground">Filter layout. 16 bands: classic resolution. 32 bands: detailed resolution.</p>
      <ParamRow columns={2}><ParamControl {...bind("bandCount")} /><ParamControl {...bind("color")} /></ParamRow>
    </ParamGroup>
    <ParamRow columns={4}>{["attackMs", "releaseMs", "gain", "mix"].map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
  </div>
}
