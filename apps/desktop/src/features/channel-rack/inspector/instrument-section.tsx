import { SampleInstrumentEditor } from "@/features/sample-instruments/editor"
import { instrumentDescriptor } from "@/features/params"
import { PluginControls } from "@/features/plugins/controls"
import { usePluginBinding } from "@/features/plugins/store"
import type { InstrumentChannel } from "@/lib/channel-source"
import { colorToCss } from "@/lib/units"

import { SoundMenu } from "../synth/sound-menu"
import { SynthEditor } from "../synth/synth-editor"
import { GenericInstrumentEditor } from "./generic-instrument-editor"
import { Section } from "./parts"

/**
 * The settings of an instrument channel: the instrument's own panel under
 * its name, with starting sounds beside the subtractive synth's panel.
 */
export function InstrumentSection({ channel }: { channel: InstrumentChannel }) {
  const plugin = usePluginBinding({ type: "instrument", channel: channel.id })
  if (plugin)
    return (
      <Section title={plugin.name}>
        <PluginControls binding={plugin} />
      </Section>
    )
  const { params } = channel.source
  return (
    <Section
      title={instrumentDescriptor(params.type).name}
      aside={
        params.type === "subtractiveSynth" ? (
          <SoundMenu params={params} />
        ) : undefined
      }
    >
      {params.type === "subtractiveSynth" ? (
        <SynthEditor
          channel={channel.id}
          params={params}
          color={colorToCss(channel.color)}
        />
      ) : ["multiSampler","samplePlayer","soundFont","padBank","pianoKeys","textVoice"].includes(params.type) ? (
        <SampleInstrumentEditor key={channel.id} channel={channel} />
      ) : (
        <GenericInstrumentEditor channel={channel.id} params={params} />
      )}
    </Section>
  )
}
