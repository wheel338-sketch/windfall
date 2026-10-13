import { useEffect, useRef, useState } from "react"
import type { Channel, EffectKind, InstrumentAsset, RackNode, RackNodeAsset, RackSampleZone } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { InstrumentEditorOwner } from "@/features/channel-rack/instruments/editor-owner"
import { LayerKickEditor } from "@/features/channel-rack/instruments/layer-kick-editor"
import { NumberEditor } from "@/features/channel-rack/instruments/number-editor"
import { ResynthesisEditor } from "@/features/channel-rack/instruments/resynthesis-editor"
import { SnapshotEditor } from "@/features/channel-rack/instruments/snapshot-editor"
import { KeyDrumEditor } from "@/features/channel-rack/instruments/key-drum-editor"
import { GenericParamEditor, instrumentDescriptor, useParamBinding, effectDescriptor, type SetParam } from "@/features/params"
import { customEditor } from "@/features/effects/effect-editor"
import { EffectAssetOwner } from "@/features/effects/asset-owner"
import type { InstrumentChannel } from "@/lib/channel-source"
import { getProjectGeneration } from "@/lib/store/replaced"
import { useProjectStore } from "@/lib/store"
import { RackAudioInput } from "./audio-input"
import { ChoiceField } from "./note-editor"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const ASSET_EDITORS: Partial<Record<EffectKind, ReturnType<typeof customEditor>>> = {
  convolver: customEditor("convolver"),
  waveshaper: customEditor("waveshaper"),
  volumeGate: customEditor("volumeGate"),
  scratch: customEditor("scratch"),
}

export function RackInstrumentEditor({ channel, node, setParam, apply, current }: {
  channel: Channel; node: RackNode; setParam: SetParam
  apply(asset: RackNodeAsset | null, gesture?: number): Promise<import("@/bindings").DispatchResult | null>
  current(): RackNode | undefined
}) {
  if (node.processor.type !== "instrument") return null
  const params = node.processor.params
  const pseudo: InstrumentChannel = { ...channel, source: { type: "instrument", params, asset: node.asset?.type === "instrument" ? node.asset.asset : undefined } }
  const read = () => {
    const found = current()
    return found?.processor.type === "instrument" ? { ...pseudo, source: { type: "instrument" as const, params: found.processor.params, asset: found.asset?.type === "instrument" ? found.asset.asset : undefined } } : undefined
  }
  const owner = { setParam, currentChannel: read, setAsset: (expected: import("@/bindings").ChannelSource, asset: InstrumentAsset | null, gesture?: number) => {
    const before = read()
    if (!before || JSON.stringify(before.source) !== JSON.stringify(expected)) return Promise.resolve(null)
    return apply(asset ? { type: "instrument", asset } : null, gesture)
  } }
  const editor = params.type === "layerKick" ? <LayerKickEditor channel={pseudo} />
    : params.type === "drumVoice" ? <KeyDrumEditor channel={pseudo} />
    : params.type === "snapshotMorph" ? <SnapshotEditor channel={pseudo} />
    : params.type === "numberVoice" ? <NumberEditor channel={pseudo} />
    : ["spectralResynth", "colorScan", "harmonicFilter", "metalVoice"].includes(params.type) ? <><ResynthesisEditor channel={pseudo} colorScan={params.type === "colorScan"} sourceOnly={["harmonicFilter", "metalVoice"].includes(params.type)} />{["harmonicFilter", "metalVoice"].includes(params.type) && <GenericParamEditor descriptor={instrumentDescriptor(params.type)} params={params} setParam={setParam} />}</>
    : <GenericParamEditor descriptor={instrumentDescriptor(params.type)} params={params} setParam={setParam} />
  return <InstrumentEditorOwner value={owner}>
    {editor}
    {["multiSampler", "samplePlayer", "padBank", "soundFont", "pianoKeys", "textVoice"].includes(params.type) && <RackBankEditor asset={node.asset ?? null} soundFont={params.type === "soundFont"} apply={apply} />}
  </InstrumentEditorOwner>
}

export function RackEffectEditor({ node, setParam, apply, commit }: { node: RackNode; setParam: SetParam; apply(asset: RackNodeAsset | null): Promise<unknown>; commit(field: string, value: unknown): void }) {
  // This component is mounted only for an effect; keep hooks unconditional.
  const params = node.processor.type === "effect" ? node.processor.params : { type: "polarity" as const, left: false, right: false }
  const descriptor = effectDescriptor(params.type)
  const bind = useParamBinding({ descriptor, params, setParam })
  if (node.processor.type !== "effect") return null
  const Editor = ASSET_EDITORS[params.type]
  const owner = { impulse: node.asset?.type === "impulse" ? node.asset.impulse : null,
    setImpulse: (impulse: import("@/bindings").ConvolverImpulse | null) => apply(impulse ? { type: "impulse", impulse } : null), commit,
    source: node.asset?.type === "audio" ? node.asset.audio : params.type === "scratch" && params.sourceId !== 0 ? { name: `Unowned legacy sample ${params.sourceId}`, sampleRate: 48000, frames: null } : null,
    setSource: (audio: import("@/bindings").RackAudioAsset | null) => apply(audio ? { type: "audio", audio } : null) }
  // Only the asset editors are used here. Other custom editors can own mixer
  // meters/routes; their generic controls remain scoped to this rack node.
  const assetEditor = ["convolver", "waveshaper", "volumeGate", "scratch"].includes(params.type)
  return <EffectAssetOwner value={owner}>{Editor && assetEditor ? <Editor effect={node.id} params={node.processor.params} bind={bind} /> : <GenericParamEditor descriptor={descriptor} params={node.processor.params} setParam={setParam} />}</EffectAssetOwner>
}

function RackBankEditor({ asset, soundFont, apply }: { asset: RackNodeAsset | null; soundFont: boolean; apply(asset: RackNodeAsset | null): Promise<unknown> }) {
  const [error, setError] = useState<string | null>(null)
  const [root, setRoot] = useState(60), [bank, setBank] = useState(asset?.type === "soundFont" ? asset.bank : 0), [program, setProgram] = useState(asset?.type === "soundFont" ? asset.program : 0)
  const request = useRef(0)
  useEffect(() => () => { ++request.current }, [])
  const zones = asset?.type === "zones" ? asset.zones : []
  const update = async (next: RackNodeAsset | null) => {
    try { if (!await apply(next)) throw new Error(t("Rack bank edit was refused; the previous bank was retained")); setError(null) }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); return null }
    return true
  }
  const mapping = (zone: RackSampleZone, patch: Partial<RackSampleZone["mapping"]>) => {
    const next = zones.map((known) => known === zone ? { ...known, mapping: { ...known.mapping, ...patch } } : known)
    void update({ type: "zones", zones: next })
  }
  return <FieldGroup>
    <Field><FieldLabel htmlFor="rack-bank-root">{t("New sample root MIDI key")}</FieldLabel><Input id="rack-bank-root" type="number" min={0} max={127} value={root} onChange={(e) => setRoot(Number(e.target.value))} /></Field>
    <RackAudioInput label={t("Bank sample")} audio={null} apply={async (audio) => {
      if (!audio) return null
      return update({ type: "zones", zones: [...zones, { audio, mapping: { sample: 0, rootKey: root, keyLow: 0, keyHigh: 127, velocityLow: 0, velocityHigh: 127, tuneCents: 0, gain: 1, pan: 0, keyFade: 0, velocityFade: 0, roundRobin: 0, chokeGroup: 0, loopMode: "off", loopStart: 0, loopEnd: 0, attackMs: 0, holdMs: 0, decayMs: 0, sustain: 1, releaseMs: 100, scaleTuning: 100 } }] })
    }} />
    {zones.map((zone, index) => <FieldGroup key={index}>
      <RackAudioInput label={message("Zone {index}", { index: plainNumber(index + 1) })} audio={zone.audio} apply={(audio) => update(audio ? { type: "zones", zones: zones.map((z) => z === zone ? { ...z, audio } : z) } : zones.length === 1 ? null : { type: "zones", zones: zones.filter((z) => z !== zone) })} />
      <ChoiceField<import("@/bindings").BankLoop> label={message("Zone {index} loop", { index: plainNumber(index + 1) })} value={zone.mapping.loopMode} choices={["off", "continuous", "untilRelease"]} onChange={(loopMode) => mapping(zone, { loopMode })} />
      {(["rootKey", "keyLow", "keyHigh", "velocityLow", "velocityHigh", "loopStart", "loopEnd", "attackMs", "releaseMs", "gain", "pan", "tuneCents"] as const).map((field) => <Field key={field}><FieldLabel htmlFor={`rack-zone-${index}-${field}`}>{t(field)}</FieldLabel><Input id={`rack-zone-${index}-${field}`} type="number" step="any" value={zone.mapping[field] ?? (field === "keyHigh" || field === "velocityHigh" ? 127 : field === "gain" ? 1 : 0)} onChange={(e) => mapping(zone, { [field]: Number(e.target.value) })} /></Field>)}
    </FieldGroup>)}
    {soundFont && <>
      <Field><FieldLabel htmlFor="rack-sf-bank">{t("SoundFont bank")}</FieldLabel><Input id="rack-sf-bank" type="number" min={0} max={16383} value={bank} onChange={(e) => { const bank = Number(e.target.value); setBank(bank); if (asset?.type === "soundFont") void update({ ...asset, bank }) }} /></Field>
      <Field><FieldLabel htmlFor="rack-sf-program">{t("SoundFont program")}</FieldLabel><Input id="rack-sf-program" type="number" min={0} max={127} value={program} onChange={(e) => { const program = Number(e.target.value); setProgram(program); if (asset?.type === "soundFont") void update({ ...asset, program }) }} /></Field>
      <Field><FieldLabel htmlFor="rack-sf-file">{t("Load or relink SoundFont")}</FieldLabel><Input id="rack-sf-file" type="file" accept=".sf2" onChange={(e) => {
        const file = e.target.files?.[0]; e.target.value = ""; if (!file) return
        const ticket = ++request.current, generation = getProjectGeneration(), revision = useProjectStore.getState().revision
        if (file.size > 64 * 1024 * 1024) { setError(t("Rack SoundFont exceeds 64 MiB")); return }
        void file.arrayBuffer().then((bytes) => {
          if (ticket !== request.current || generation !== getProjectGeneration()) return
          if (revision !== useProjectStore.getState().revision) throw new Error(t("Project changed during SoundFont import; choose the file again"))
          return update({ type: "soundFont", name: file.name, bank, program, bytes: Array.from(new Uint8Array(bytes)) })
        }).catch((cause: unknown) => { if (ticket === request.current && generation === getProjectGeneration()) setError(cause instanceof Error ? cause.message : String(cause)) })
      }} /><FieldDescription>{asset?.type === "soundFont" ? message(asset.bytes ? "{name}: embedded in rack presets" : "{name}: missing; choose a replacement file", { name: asset.name }) : t("SoundFont bytes and the selected preset travel with the rack")}</FieldDescription></Field>
    </>}
    <Button variant="outline" disabled={!asset} onClick={() => { ++request.current; void update(null) }}>{t("Clear rack bank")}</Button>
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </FieldGroup>
}
