import { DashboardEditor } from "@/features/internal-controllers/dashboard"
import { SpecialOutputEditor } from "@/features/sample-instruments/special-output-editor"
import { SampleInstrumentEditor } from "@/features/sample-instruments/editor"
import { instrumentDescriptor } from "@/features/params"
import { PluginControls } from "@/features/plugins/controls"
import { usePluginBinding } from "@/features/plugins/store"
import type { InstrumentChannel } from "@/lib/channel-source"
import { useProjectGeneration } from "@/lib/store/replaced"
import { colorToCss } from "@/lib/units"

import { ResynthesisEditor } from "../instruments/resynthesis-editor"
import { NumberEditor } from "../instruments/number-editor"
import { MetalEditor } from "../instruments/metal-editor"
import { HarmonicFilterEditor } from "../instruments/harmonic-filter-editor"
import { SnapshotEditor } from "../instruments/snapshot-editor"
import { LayerKickEditor } from "../instruments/layer-kick-editor"
import { KeyDrumEditor } from "../instruments/key-drum-editor"
import { DrumRackEditor } from "../instruments/drum-rack-editor"
import { MidiOutputEditor } from "../instruments/midi-output-editor"
import { MembraneEditor } from "../instruments/membrane-editor"

import { SoundMenu } from "../synth/sound-menu"
import { SynthEditor } from "../synth/synth-editor"
import { POLYSYNTH_PANELS, PolysynthEditor } from "../synth/polysynth-editor"
import { SamplePlaybackEditor } from "@/features/sample-playback/editor"
import { GenericInstrumentEditor } from "./generic-instrument-editor"
import { Section } from "./parts"

/**
 * The settings of an instrument channel: the instrument's own panel under
 * its name, with starting sounds beside the subtractive synth's panel.
 */
export function InstrumentSection({ channel }: { channel: InstrumentChannel }) {
  const generation = useProjectGeneration()
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
      key={`${channel.id}:${generation}`}
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
      ) : POLYSYNTH_PANELS[params.type] ? (
        <PolysynthEditor channel={channel.id} params={params} />
      ) : params.type === "colorScan" ? (
        <ResynthesisEditor channel={channel} colorScan />
      ) : params.type === "spectralResynth" ? (
        <ResynthesisEditor channel={channel} />
      ) : params.type === "numberVoice" ? (
        <NumberEditor channel={channel} />
      ) : params.type === "metalVoice" ? (
        <MetalEditor channel={channel.id} params={params} />
      ) : params.type === "harmonicFilter" ? (
        <HarmonicFilterEditor channel={channel.id} params={params} />
      ) : params.type === "snapshotMorph" ? (
        <SnapshotEditor channel={channel} />
      ) : params.type === "layerKick" ? (
        <LayerKickEditor channel={channel} />
      ) : params.type === "drumVoice" ? (
        <KeyDrumEditor channel={channel} />
      ) : params.type === "drumRack" ? (
        <DrumRackEditor channel={channel.id} params={params} />
      ) : params.type === "midiOut" ? (
        <div className="flex flex-col gap-3"><MidiOutputEditor channel={channel} /><SpecialOutputEditor key={`${generation}:${channel.id}`} /></div>
      ) : params.type === "membrane" ? (
        <MembraneEditor channel={channel.id} params={params} />
      ) : ["sliceMap", "sliceDeck", "grainCloud", "waveRide"].includes(params.type) ? (
        <SamplePlaybackEditor key={`${generation}:${channel.id}:${params.type}`} channel={channel} />
      ) : params.type === "dashboard" ? (
        <DashboardEditor key={channel.id} channel={channel.id} params={params} />
      ) : ["multiSampler","samplePlayer","soundFont","padBank","pianoKeys","textVoice"].includes(params.type) ? (
        <SampleInstrumentEditor key={`${generation}:${channel.id}:${params.type}`} channel={channel} />
      ) : (
        <div className="flex flex-col gap-3">
        <GenericInstrumentEditor channel={channel.id} params={params} />
        </div>
      )}
    </Section>
  )
}
