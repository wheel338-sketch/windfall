import { createContext, useContext } from "react"
import type { ConvolverImpulse, EffectParams, RackAudioAsset } from "@/bindings"

export const EffectAssetOwner = createContext<{
  impulse: ConvolverImpulse | null
  setImpulse(impulse: ConvolverImpulse | null): Promise<unknown>
  commit(field: string, value: unknown): void
  source?: RackAudioAsset | null
  setSource?(audio: RackAudioAsset | null): Promise<unknown>
} | null>(null)

export function useEffectAssetCommit(effect: number, track: number | undefined, kind: EffectParams["type"], fallback: (effect: number, track: number | undefined, kind: EffectParams["type"], field: string, value: unknown) => void) {
  const owner = useContext(EffectAssetOwner)
  return (field: string, value: unknown) => owner ? owner.commit(field, value) : fallback(effect, track, kind, field, value)
}
