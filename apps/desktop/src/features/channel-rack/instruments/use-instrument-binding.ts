import { useCallback } from "react"
import type { ChannelId, InstrumentParams } from "@/bindings"
import { useParamAutomation } from "@/features/automation/live"
import { instrumentDescriptor, useParamBinding, type SetParam } from "@/features/params"
import { dispatch } from "@/lib/store"

export function useInstrumentBinding(channel: ChannelId, params: InstrumentParams) {
  const setParam: SetParam = useCallback(
    (param, value, gesture) => dispatch({ type: "setInstrumentParam", channel, param, value }, gesture), [channel]
  )
  const automation = useParamAutomation((param) => ({ type: "instrumentParam", channel, param }), String(channel))
  return useParamBinding({ descriptor: instrumentDescriptor(params.type), params, setParam, ...automation })
}
