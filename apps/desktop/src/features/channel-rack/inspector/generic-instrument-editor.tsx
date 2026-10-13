import { useLanguageStore } from "@/lib/i18n"
import { useCallback } from "react"

import type { ChannelId, InstrumentParams } from "@/bindings"
import { useParamAutomation } from "@/features/automation/live"
import {
  GenericParamEditor,
  instrumentDescriptor,
  type SetParam,
} from "@/features/params"
import { dispatch } from "@/lib/store"

/** Descriptor controls for a built-in instrument without a custom panel. */
export function GenericInstrumentEditor({
  channel,
  params,
}: {
  channel: ChannelId
  params: InstrumentParams
}) {
  useLanguageStore((state) => state.language)
  const setParam: SetParam = useCallback(
    (param, value, gesture) =>
      dispatch({ type: "setInstrumentParam", channel, param, value }, gesture),
    [channel]
  )
  const automation = useParamAutomation(
    (param) => ({ type: "instrumentParam", channel, param }),
    String(channel)
  )
  return (
    <GenericParamEditor
      descriptor={instrumentDescriptor(params.type)}
      params={params}
      setParam={setParam}
      {...automation}
    />
  )
}
