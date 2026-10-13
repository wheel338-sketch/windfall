import { useCallback } from "react"
import type { ChannelId, InstrumentParams } from "@/bindings"
import { useParamAutomation } from "@/features/automation/live"
import { instrumentDescriptor, ParamControl, ParamGroup, ParamRow, useParamBinding, type SetParam } from "@/features/params"
import { dispatch } from "@/lib/store"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { PackBrowser } from "./pack-browser"
import { translatedText as t, message } from "@/lib/i18n"

const amp = ["envelope.attackMs", "envelope.decayMs", "envelope.sustain", "envelope.releaseMs"]
const filterEnv = ["filterEnvelope.attackMs", "filterEnvelope.decayMs", "filterEnvelope.sustain", "filterEnvelope.releaseMs"]
const operators: (readonly [string, readonly string[]])[] = Array.from({ length: 6 }, (_, i) => [`Operator ${i + 1}`, ["ratio", "detuneCents", "level", "feedback", "keyScale", "waveform", "envelope.attackMs", "envelope.decayMs", "envelope.sustain", "envelope.releaseMs"].map((field) => `operators.${i}.${field}`)] as const)
export const POLYSYNTH_PANELS: Record<string, readonly (readonly [string, readonly string[]])[]> = {
  packSynth: [
    ["Performance macros", ["tone", "shape", "motion", "attack", "decay", "width", "air", "crunch"]],
    ["Space and voices", ["echo", "space", "glideMs", "polyphony", "gain"]],
  ],
  matrixSynth: [
    ["Matrix modulation", ["lfoRateHz", "lfoPitch", "lfoIndex", "lfoFilter"]], ...operators,
    ["Carrier outputs", Array.from({ length: 6 }, (_, i) => `output.${i}`)],
    ["Subtractive stage", ["filterMix", "cutoffHz", "resonance", "drive", "filterAmount", "keyTrack"]],
    ["Amp contour", amp], ["Filter contour", filterEnv],
    ["Voices and effects", ["unison", "detuneCents", "spread", "glideMs", "polyphony", "chorus", "delay", "reverb", "gain"]],
  ],
  hybridSeq: [
    ["FM and subtractive graph", ["algorithm", "index", "sawMix"]], ...operators,
    ["Sequence clock", ["sequencer", "length", "division", "gate"]],
    ["Filter", ["cutoffHz", "resonance", "drive", "filterAmount", "keyTrack"]],
    ["Amp contour", amp], ["Filter contour", filterEnv], ["LFO", ["lfoRateHz", "lfoFilter"]],
    ["Voices and effects", ["unison", "detuneCents", "spread", "glideMs", "polyphony", "chorus", "delay", "reverb", "gain"]],
  ],
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
  if (params.type === "pocketSynth") return <Tabs defaultValue="Oscillator" aria-label={t("Pocket synth pages")}>
    <TabsList>{POLYSYNTH_PANELS.pocketSynth.map(([title]) => <TabsTrigger key={title} value={title}>{title}</TabsTrigger>)}</TabsList>
    {POLYSYNTH_PANELS.pocketSynth.map(([title, ids]) => <TabsContent key={title} value={title}>
      <ParamGroup title={title}><ParamRow>{ids.map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow></ParamGroup>
    </TabsContent>)}
  </Tabs>
  return <div data-slot="polysynth-editor" role="group" aria-label={message("{descriptorName} settings", { descriptorName: descriptor.name })} className="flex flex-col gap-2">
    {params.type === "packSynth" && <PackBrowser bind={bind} />}
    {params.type === "matrixSynth" && (["fm", "ring"] as const).map((matrix) => <ParamGroup key={matrix} title={matrix === "fm" ? "FM routing matrix" : "Ring routing matrix"}>
      <p className="text-muted-foreground">{t("Rows receive modulation from the operators shown above each column.")}</p>
      <div className="overflow-auto"><table aria-label={matrix === "fm" ? t("FM operator routing") : t("Ring operator routing")}>
        <thead><tr><th scope="col">{t("Destination / Source")}</th>{Array.from({ length: 6 }, (_, source) => <th key={source} scope="col">Op {source + 1}</th>)}</tr></thead>
        <tbody>{Array.from({ length: 6 }, (_, destination) => <tr key={destination}><th scope="row">Op {destination + 1}</th>
          {Array.from({ length: 6 }, (_, source) => <td key={source}><ParamControl size="sm" {...bind(`${matrix}.${destination}.${source}`)} /></td>)}
        </tr>)}</tbody>
      </table></div>
    </ParamGroup>)}
    {POLYSYNTH_PANELS[params.type].map(([title, ids]) => <ParamGroup key={title} title={title}>
      <ParamRow>{ids.map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
    </ParamGroup>)}
    {params.type === "hybridSeq" && <ParamGroup title="Sixteen-step sequence">
      <div className="overflow-auto"><table aria-label={t("Hybrid note sequence")}>
        <thead><tr><th scope="col">{t("Step")}</th><th scope="col">{t("Pitch")}</th><th scope="col">{t("Velocity")}</th><th scope="col">{t("Enabled")}</th></tr></thead>
        <tbody>{Array.from({ length: 16 }, (_, step) => <tr key={step}>
          <th scope="row">{step + 1}</th>
          {["pitch", "velocity", "enabled"].map((field) => <td key={field}><ParamControl size="sm" {...bind(`steps.${step}.${field}`)} /></td>)}
        </tr>)}</tbody>
      </table></div>
    </ParamGroup>}
  </div>
}
