import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Field, FieldLabel, FieldDescription } from "@/components/ui/field"
import { ParamControl, ParamGroup, ParamRow, type EffectParamsOf } from "@/features/params"
import { dispatch, useProjectStore } from "@/lib/store/project"
import type { EditorProps } from "../editor-props"
export function PitchShiftEditor({ bind }: EditorProps<EffectParamsOf<"pitchShift">>) {
  return <div className="flex flex-col gap-3" data-slot="pitch-shift-editor">
    <ParamRow columns={4}>{["semitones", "formantSemitones", "mode", "mix"].map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
    <p className="text-xs text-muted-foreground">Voice mode separates excitation and an adaptive vocal envelope. Full mix keeps the granular path at neutral formants. Formant estimation is approximate. Fixed centre latency: 1026 frames; shifted grains span 2-2050 frames.</p>
  </div>
}
export function PitchCorrectEditor({ effect, track, params, bind }: EditorProps<EffectParamsOf<"pitchCorrect">>) {
  const channels = useProjectStore((state) => state.project.channels)
  return <div className="flex flex-col gap-3" data-slot="pitch-correct-editor">
    <ParamGroup title="Scale correction"><ParamRow columns={3}>{["root", "scale", "tuningHz", "speedMs", "amount", "mix"].map((id) => <ParamControl key={id} {...bind(id)} />)}</ParamRow></ParamGroup>
    <Field>
      <FieldLabel htmlFor={`harmony-${effect}`}>Harmony MIDI source</FieldLabel>
      <Select value={String(params.harmonyChannel ?? 0)} onValueChange={(value) => {
        if (track === undefined || value === null) return
        void dispatch({ type: "setEffectParams", track, effect, params: { ...params, harmonyChannel: Number(value) } })
      }}>
        <SelectTrigger id={`harmony-${effect}`} aria-label="Harmony MIDI source"><SelectValue>{params.harmonyChannel ? channels.find((channel) => channel.id === params.harmonyChannel)?.name ?? "Unavailable source" : "Off"}</SelectValue></SelectTrigger>
        <SelectContent><SelectGroup>
          <SelectItem value="0">Off</SelectItem>
          {channels.map((channel) => <SelectItem key={channel.id} value={String(channel.id)}>{channel.name}</SelectItem>)}
          {!!params.harmonyChannel && !channels.some((channel) => channel.id === params.harmonyChannel) && <SelectItem value={String(params.harmonyChannel)}>Unavailable source</SelectItem>}
        </SelectGroup></SelectContent>
      </Select>
      <FieldDescription>Held notes from this channel tune added voices. A missing source is silent.</FieldDescription>
    </Field>
    <ParamControl {...bind("harmonyMix")} />
    <p className="text-xs text-muted-foreground">The four lowest held keys from this channel tune added voices, including live MIDI input. Voices beyond two octaves from the detected input are limited to that range. Use a silent source channel when only its MIDI is needed. Monophonic detector: 60-1000 Hz. At 48 kHz, acquisition window: 2048 frames; fixed audio centre delay: 3074 frames.</p>
  </div>
}
