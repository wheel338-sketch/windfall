import { ParamControl, ParamRow, type ParamBinder } from "@/features/params"

/** Synced motion keeps the phase running; one cycle lasts this many beats. */
export function Motion({ bind, rate = "rateHz", disabled = false }: {
  bind: ParamBinder
  rate?: string
  disabled?: boolean
}) {
  const sync = bind.value("sync") >= 0.5
  return (
    <ParamRow columns={3}>
      <ParamControl {...bind(rate)} disabled={disabled || sync || bind.disabled} />
      <ParamControl {...bind("sync")} disabled={disabled || bind.disabled} />
      <ParamControl {...bind("cycleBeats")} disabled={disabled || !sync || bind.disabled}
        description="Quarter-note beats in one modulation cycle; tempo edits retain the running phase" />
    </ParamRow>
  )
}

export const CIRCUIT_LAYOUT = "grid grid-cols-[repeat(auto-fit,minmax(11.5rem,1fr))] items-start gap-2"
