import type { InstrumentChannel } from "@/lib/channel-source"
import { useEffect, useState } from "react"
import type { MidiHardwareState } from "@/bindings"
import { backend } from "@/lib/ipc"
import { dispatch, useProjectStore } from "@/lib/store"
import { ControllerChoice } from "@/features/controllers/links-panel"
import { instrumentDescriptor, ParamControl, ParamGroup, ParamRow } from "@/features/params"
import { FieldDescription } from "@/components/ui/field"
import { useInstrumentBinding } from "./use-instrument-binding"

export function MidiOutputEditor({ channel }: { channel: InstrumentChannel }) {
  const [hardware, setHardware] = useState<MidiHardwareState | null>(null)
  const project = useProjectStore((state) => state.project)
  const bind = useInstrumentBinding(channel.id, channel.source.params)
  useEffect(() => {
    let disposed = false
    void backend.midiHardwareState().then((state) => { if (!disposed) setHardware(state) }).catch(() => { /* Unavailable ports remain disconnected. */ })
    return () => { disposed = true }
  }, [])
  const params = channel.source.params
  if (params.type !== "midiOut") return null
  const destination = params.pluginDestination ?? 0
  const plugins = (project.plugins ?? []).flatMap((plugin) => plugin.target.type === "instrument" && plugin.target.channel !== channel.id ? [{ value: String(plugin.target.channel), label: plugin.name }] : [])
  const port = params.outputPort ?? 0
  const ids = [hardware?.settings.output, ...(hardware?.settings.extraOutputs ?? [])]
  function route(change: Partial<typeof params>) {
    void dispatch({ type: "setInstrumentParams", channel: channel.id, params: { ...params, ...change } })
  }
  return <div className="flex flex-col gap-3" role="group" aria-label="MIDI output routing">
    <ControllerChoice<string> label="MIDI destination" value={String(destination)} choices={[{ value: "-1", label: "Disconnected" }, { value: "0", label: "Hardware output" }, ...plugins]} change={(value) => route({ pluginDestination: Number(value) })} />
    {destination === 0 && <><ControllerChoice<string> label="MIDI output port" value={String(port)} choices={Array.from({ length: 8 }, (_, index) => {
      const id = ids[index]
      return { value: String(index), label: `${index === 0 ? "Main output" : `Output slot ${index}`}: ${hardware?.outputs.find((output) => output.id === id)?.name ?? "not connected"}` }
    })} change={(value) => route({ outputPort: Number(value) })} /><FieldDescription>Configure output slots in MIDI settings. Missing ports stay disconnected.</FieldDescription></>}
    <ParamGroup title="MIDI channel and messages"><ParamRow>{instrumentDescriptor("midiOut").params.slice(0, 21).map((info) => <ParamControl key={info.id} {...bind(info.id)} label={info.name} />)}</ParamRow></ParamGroup>
  </div>
}
