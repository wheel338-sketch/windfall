import type { ZoneModulation, SampleZone } from "@/bindings"
import { FieldDescription, FieldGroup } from "@/components/ui/field"
import { Input } from "@/components/ui/input"

export const DEFAULT_ZONE_MODULATION: ZoneModulation = {
  cutoffHz: 20000, delayMs: 0, attackMs: 0, holdMs: 0, decayMs: 200, sustain: 0,
  releaseMs: 100, envelopePitchCents: 0, envelopeFilterCents: 0, envelopeAmplitude: 0,
  lfos: [
    { rateHz: 5, delayMs: 0, phase: 0, pitchCents: 0, filterCents: 0, amplitude: 0 },
    { rateHz: 5, delayMs: 0, phase: 0, pitchCents: 0, filterCents: 0, amplitude: 0 },
  ],
}

function Control({ label, value, min, max, commit }: {
  label: string; value: number; min: number; max: number; commit: (value: number) => void
}) {
  return <Input key={value} aria-label={label} type="number" step="any" min={min} max={max}
    defaultValue={value} onBlur={event=>{
      const next=event.currentTarget.valueAsNumber
      if(Number.isFinite(next) && next!==value)commit(next)
    }} />
}

export function ZoneModulationEditor({ zone, index, commit }: {
  zone: SampleZone; index: number; commit: (modulation: ZoneModulation) => void
}) {
  const mod=zone.modulation ?? DEFAULT_ZONE_MODULATION
  const prefix=`Zone ${index+1}`
  const fields = [
    ["cutoffHz", "Filter cutoff Hz", 20, 20000],
    ["delayMs", "Mod envelope delay ms", 0, 100000],
    ["attackMs", "Mod envelope attack ms", 0, 100000],
    ["holdMs", "Mod envelope hold ms", 0, 100000],
    ["decayMs", "Mod envelope decay ms", 0, 100000],
    ["sustain", "Mod envelope sustain", 0, 1],
    ["releaseMs", "Mod envelope release ms", 0, 100000],
    ["envelopePitchCents", "Envelope to pitch cents", -12000, 12000],
    ["envelopeFilterCents", "Envelope to filter cents", -12000, 12000],
    ["envelopeAmplitude", "Envelope to amplitude", -1, 1],
  ] as const
  return <FieldGroup>
    <FieldDescription>Modulation group {zone.modulationGroup ?? 0}. Group 0 is independent; other groups share routes. Envelope and two sine LFOs can each drive pitch, filter and amplitude.</FieldDescription>
    {fields.map(([key,label,min,max])=><Control key={key} label={`${prefix} ${label}`} value={mod[key]}
      min={min} max={max} commit={value=>commit({...mod,[key]:value})} />)}
    {mod.lfos.map((lfo, lfoIndex)=>{
      const controls = [["rateHz","rate Hz",0,100],["delayMs","delay ms",0,100000],["phase","phase cycles",0,1],
        ["pitchCents","to pitch cents",-12000,12000],["filterCents","to filter cents",-12000,12000],["amplitude","to amplitude",-1,1]] as const
      return <div key={lfoIndex} className="flex flex-col gap-1">
        {controls.map(([key,label,min,max])=><Control key={key} label={`${prefix} LFO ${lfoIndex+1} ${label}`} value={lfo[key]}
          min={min} max={max} commit={value=>{
            const lfos: ZoneModulation["lfos"]=[mod.lfos[0],mod.lfos[1]]
            lfos[lfoIndex]={...lfo,[key]:value}
            commit({...mod,lfos})
          }} />)}
      </div>
    })}
  </FieldGroup>
}
