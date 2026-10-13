import { createContext, useContext } from "react"
import type { ChannelSource, DispatchResult, InstrumentAsset } from "@/bindings"
import type { SetParam } from "@/features/params"
import type { InstrumentChannel } from "@/lib/channel-source"
import { dispatch } from "@/lib/store"

/** The same real editor may be owned by a channel or by a nested rack node. */
export const InstrumentEditorOwner = createContext<{
  setParam: SetParam
  currentChannel(): InstrumentChannel | undefined
  setAsset(expected: ChannelSource, asset: InstrumentAsset | null, gesture?: number): Promise<DispatchResult | null>
} | null>(null)

export function useInstrumentAssetDispatch() {
  const owner = useContext(InstrumentEditorOwner)
  return (command: { type: "setInstrumentAsset"; channel: number; expected: ChannelSource; asset: InstrumentAsset | null }, gesture?: number) =>
    owner ? command.expected ? owner.setAsset(command.expected, command.asset, gesture) : Promise.resolve(null) : dispatch(command, gesture)
}
