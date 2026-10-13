import { useCallback } from "react"
import type { ChannelId, InstrumentParams } from "@/bindings"
import { instrumentDescriptor, ParamControl, useParamBinding, type SetParam } from "@/features/params"
import { useParamAutomation } from "@/features/automation/live"
import { dispatch } from "@/lib/store/project"
import { SurfacePanel } from "./surface"
import { SourceLinks } from "./links"
import { message, plainNumber, translatedText as t } from "@/lib/i18n"

export function DashboardEditor({ channel, params }: { channel: ChannelId; params: Extract<InstrumentParams, { type: "dashboard" }> }) {
  const setParam: SetParam = useCallback((param, value, gesture) => dispatch({ type: "setInstrumentParam", channel, param, value }, gesture), [channel])
  const automation = useParamAutomation((param) => ({ type: "instrumentParam", channel, param }), `dashboard:${channel}`)
  const bind = useParamBinding({ descriptor: instrumentDescriptor("dashboard"), params, setParam, ...automation })
  return <div className="flex flex-col gap-3"><SurfacePanel controller={channel} bind={bind} dashboard /><div className="flex flex-wrap gap-2"><ParamControl {...bind("channel")} />{Array.from({ length: 8 }, (_, output) => <ParamControl key={output} {...bind(`controllers.${output}.number`)} label={message("MIDI CC {output} (-1 disables)", { output: plainNumber(output + 1) })} />)}</div><p className="text-xs text-muted-foreground">{t("Each control can send an assigned MIDI CC and link to an internal destination. MIDI output uses the configured MIDI hardware output port.")}</p><SourceLinks controller={channel} /></div>
}
