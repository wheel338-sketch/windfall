import { useCallback } from "react"
import type { ChannelId, InstrumentParams } from "@/bindings"
import { useParamAutomation } from "@/features/automation/live"
import { instrumentDescriptor, ParamControl, ParamGroup, ParamRow, useParamBinding, type SetParam } from "@/features/params"
import { dispatch } from "@/lib/store"

const amp = ["envelope.attackMs", "envelope.decayMs", "envelope.sustain", "envelope.releaseMs"]
const filterEnv = ["filterEnvelope.attackMs", "filterEnvelope.decayMs", "filterEnvelope.sustain", "filterEnvelope.releaseMs"]
export const POLYSYNTH_PANELS: Record<string, readonly (readonly [string, readonly string[]])[]> = {
  vintageSaw: [
    ["Charging saw oscillator", ["pulse", "sub", "pulseWidth", "pwm", "drift", "asymmetry", "pitchDrop", "pitchDecayMs"]],
    ["Saturating vintage filter", ["cutoffHz", "resonance", "drive", "filterAmount", "keyTrack"]],
    ["Amp contour", amp], ["Filter contour", filterEnv], ["LFO", ["lfoRateHz", "lfoFilter"]],
    ["Voices and effects", ["unison", "detuneCents", "spread", "glideMs", "polyphony", "chorus", "delay", "phaser", "gain"]],
  ],
  compactSynth: [
    ["Quick oscillators", ["waveA", "waveB", "mix", "coarse", "fine", "pulseWidth", "pwm", "ring"]],
    ["Multimode filter", ["response", "cutoffHz", "resonance", "drive", "filterAmount", "keyTrack"]],
    ["Amp contour", amp], ["Filter contour", filterEnv], ["Motion", ["lfoRateHz", "lfoFilter"]],
    ["Voices", ["unison", "detuneCents", "spread", "glideMs", "polyphony", "chorus", "gain"]],
  ],
  dualPoly: [
    ["Dual oscillator banks", ["waveA", "waveB", "balance", "coarseB", "fineB", "pulseWidth", "pwm", "crossMod", "sub", "noise", "drift"]],
    ["Variable ladder", ["response", "cutoffHz", "resonance", "drive", "filterAmount", "keyTrack"]],
    ["Filter contour", filterEnv], ["Amp contour", amp], ["LFO", ["lfoRateHz", "lfoFilter"]],
    ["Voices and space", ["unison", "detuneCents", "spread", "glideMs", "polyphony", "chorus", "delay", "reverb", "gain"]],
  ],
  vintagePoly: [
    ["DCO and divider", ["saw", "pulse", "sub", "noise", "pulseWidth", "pwm", "drift"]],
    ["Resonant ladder", ["cutoffHz", "resonance", "highpassHz", "filterAmount", "keyTrack"]],
    ["Filter contour", filterEnv], ["Amp contour", amp],
    ["LFO", ["lfoRateHz", "lfoFilter"]],
    ["Voices and chorus", ["unison", "detuneCents", "spread", "glideMs", "polyphony", "chorus", "gain"]],
  ],
}

/** Deliberate signal-flow panels. Every control retains history and automation binding. */
export function PolysynthEditor({ channel, params }: { channel: ChannelId; params: InstrumentParams }) {
  const descriptor = instrumentDescriptor(params.type)
  const setParam: SetParam = useCallback((param, value, gesture) => dispatch({ type: "setInstrumentParam", channel, param, value }, gesture), [channel])
  const automation = useParamAutomation((param) => ({ type: "instrumentParam", channel, param }), String(channel))
  const bind = useParamBinding({ descriptor, params, setParam, ...automation })
  return <div data-slot="polysynth-editor" role="group" aria-label={`${descriptor.name} settings`} className="flex flex-col gap-2">
    {POLYSYNTH_PANELS[params.type].map(([title, ids]) => <ParamGroup key={title} title={title}>
      <ParamRow>{ids.map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
    </ParamGroup>)}
  </div>
}
