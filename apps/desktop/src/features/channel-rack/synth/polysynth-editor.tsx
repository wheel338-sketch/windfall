import { useCallback } from "react"
import type { ChannelId, InstrumentParams } from "@/bindings"
import { useParamAutomation } from "@/features/automation/live"
import { instrumentDescriptor, ParamControl, ParamGroup, ParamRow, useParamBinding, type SetParam } from "@/features/params"
import { dispatch } from "@/lib/store"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"

const amp = ["envelope.attackMs", "envelope.decayMs", "envelope.sustain", "envelope.releaseMs"]
const filterEnv = ["filterEnvelope.attackMs", "filterEnvelope.decayMs", "filterEnvelope.sustain", "filterEnvelope.releaseMs"]
export const POLYSYNTH_PANELS: Record<string, readonly (readonly [string, readonly string[]])[]> = {
  grooveSynth: [
    ["Groove engine", ["mode", "mix", "coarse", "ratio", "shape", "index"]],
    ["Rhythm and motion", ["sync", "division", "gate", "motion", "lfoRateHz", "lfoFilter"]],
    ["Filter", ["cutoffHz", "resonance", "drive", "filterAmount", "keyTrack"]],
    ["Amp contour", amp], ["Filter contour", filterEnv],
    ["Voices and effects", ["unison", "detuneCents", "spread", "glideMs", "polyphony", "chorus", "delay", "phaser", "reverb", "gain"]],
  ],
  pocketSynth: [
    ["Oscillator", ["waveform", "pulseWidth", "sub", "drive"]],
    ["Filter", ["cutoffHz", "resonance", "filterAmount", "keyTrack"]],
    ["Envelope", amp], ["Motion", ["lfoRateHz", "vibrato", "glideMs", "polyphony"]],
    ["Effects", ["chorus", "delay", "gain"]],
  ],
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
  if (params.type === "pocketSynth") return <Tabs defaultValue="Oscillator" aria-label="Pocket synth pages">
    <TabsList>{POLYSYNTH_PANELS.pocketSynth.map(([title]) => <TabsTrigger key={title} value={title}>{title}</TabsTrigger>)}</TabsList>
    {POLYSYNTH_PANELS.pocketSynth.map(([title, ids]) => <TabsContent key={title} value={title}>
      <ParamGroup title={title}><ParamRow>{ids.map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow></ParamGroup>
    </TabsContent>)}
  </Tabs>
  return <div data-slot="polysynth-editor" role="group" aria-label={`${descriptor.name} settings`} className="flex flex-col gap-2">
    {POLYSYNTH_PANELS[params.type].map(([title, ids]) => <ParamGroup key={title} title={title}>
      <ParamRow>{ids.map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
    </ParamGroup>)}
  </div>
}
