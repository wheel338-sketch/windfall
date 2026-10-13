import type { ChannelId, InstrumentParams } from "@/bindings"
import { ParamControl, ParamGroup, ParamRow } from "@/features/params"
import { useInstrumentBinding } from "./use-instrument-binding"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

type Params = Extract<InstrumentParams, { type: "membrane" }>
export function MembraneEditor({ channel, params }: { channel: ChannelId; params: Params }) {
  const bind = useInstrumentBinding(channel, params)
  return <div role="group" aria-label={t("Membrane settings")} className="flex flex-col gap-3">
    <ParamGroup title="Membrane and cavity">
      <svg viewBox="0 0 120 100" role="img" aria-label={message("Strike at {strikePosition} percent of the drum radius", { strikePosition: plainNumber(Math.round(params.strikePosition * 100)) })} className="h-24 text-muted-foreground">
        <circle cx="60" cy="50" r="42" fill="none" stroke="currentColor" />
        <circle cx="60" cy="50" r="24" fill="none" stroke="currentColor" strokeDasharray="3 3" />
        <circle cx={60 + 42 * params.strikePosition} cy="50" r="4" fill="currentColor" />
      </svg>
      <ParamRow>{["strikePosition", "damping", "body", "tone", "snap"].map(id => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
    </ParamGroup>
    <ParamGroup title="Tuning and decay"><ParamRow>{["pitchHz", "decayMs", "pitchDropSemitones", "pitchDecayMs", "level"].map(id => <ParamControl key={id} {...bind(id)} />)}</ParamRow></ParamGroup>
  </div>
}
