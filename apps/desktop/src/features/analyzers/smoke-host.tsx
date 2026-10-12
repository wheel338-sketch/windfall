import { invoke } from "@tauri-apps/api/core"
import { useEffect, useState } from "react"

import { EffectEditor } from "@/features/effects/effect-editor"
import { StripWaveform } from "@/features/mixer/waveform-meter"
import { LargeClock } from "@/features/stage/large-clock"
import { LargeMasterMeter } from "@/features/stage/large-master-meter"
import { backend } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"

import { useAnalyzerSelection } from "./feed"
import { AnalyzerStatusLine } from "./status"
import type { AnalyzerSource } from "./types"
import { Oscilloscope, PeakMeter, Spectrogram, SpectrumCurve, Vectorscope } from "./views"

type Fixture = {
  project: string
  tracks: [number, number]
  limiter: number
  tuner: number
  eq: number
}
type EffectView = "limiter" | "tuner" | "eq" | null
type SmokeApi = {
  prepare(): Promise<Fixture>
  control(operation: string, value?: number): Promise<unknown>
  select(source: "left" | "right" | "mix"): void
  effect(view: EffectView): void
  deleteSource(): Promise<void>
  read(): ReturnType<typeof readDisplays>
}
declare global { interface Window { __windfallAnalyzerSmoke?: SmokeApi } }

/** Reads what the mounted views actually wrote, rather than echoing IPC. */
export function readDisplays(root: HTMLElement | null) {
  if (!root) throw new Error("Analyzer smoke views are not mounted")
  const text = (selector: string) => root.querySelector(selector)?.textContent ?? null
  const data = (selector: string) => [...root.querySelectorAll<HTMLElement>(selector)].map((element) => ({
    ...element.dataset,
    label: element.getAttribute("aria-label"),
  }))
  return {
    clock: { position: text("[data-slot=large-clock] [data-part=position]"), time: text("[data-slot=large-clock] [data-part=time]") },
    master: { left: text("[data-slot=large-master-meter] [data-side=left]"), right: text("[data-slot=large-master-meter] [data-side=right]") },
    meters: data("[data-slot=scope-meter] [data-side]"),
    canvases: data("[data-slot=analyzer-canvas]"),
    waveforms: data("[data-slot=mixer-waveform]"),
    limiter: data("[data-slot=limiter-history]"),
    eq: data("[data-slot=eq-live-spectrum]"),
    tuner: { note: text("[data-slot=tuner-note]"), cents: text("[data-slot=tuner-cents]"), values: data("[data-slot=tuner-editor]") },
    statuses: data("[data-slot=analyzer-status]"),
  }
}

function SmokeViews({ fixture, source, effect }: { fixture: Fixture | null; source: AnalyzerSource | null; effect: EffectView }) {
  useAnalyzerSelection("scope", source ? { source, want: { scope:true, spectrum:true, vector:true, envelope:true } } : null)
  const project = useProjectStore((state) => state.project)
  const selected = fixture && effect ? fixture[effect] : null
  const track = project.mixer.tracks.find((track) => track.effects.some((slot) => slot.id === selected))
  const slot = track?.effects.find((slot) => slot.id === selected)
  return <section data-slot="analyzer-smoke" aria-label="Analyzer smoke displays" className="absolute inset-0 overflow-auto bg-chassis p-4">
    <h1 className="mb-3 text-lg text-foreground">Analyzer smoke displays</h1>
    <div className="grid grid-cols-2 gap-4">
      <LargeClock /><LargeMasterMeter />
      <div className="col-span-2"><PeakMeter slot="scope" floorDb={-90} /><AnalyzerStatusLine slot="scope" /></div>
      <div className="h-36"><Oscilloscope slot="scope" span={1024} triggered={false} /></div>
      <div className="h-36"><SpectrumCurve slot="scope" floorDb={-90} /></div>
      <div className="h-36"><Spectrogram slot="scope" floorDb={-90} /></div>
      <div className="h-36"><Vectorscope slot="scope" /></div>
      {fixture?.tracks.map((id) => <div key={id} className="h-12"><StripWaveform id={id} active vertical={false} wide /></div>)}
      {slot && track && <div className="col-span-2"><EffectEditor trackId={track.id} slot={slot} /></div>}
    </div>
  </section>
}

/** Both gates are native: debug_assertions AND WINDFALL_ANALYZER_SMOKE=1.
 * The command also requires an explicit isolated configuration directory.
 * Release apps never mount these views or install a window control bridge. */
export function AnalyzerSmokeHost() {
  const [enabled, setEnabled] = useState(false)
  const [fixture, setFixture] = useState<Fixture | null>(null)
  const [source, setSource] = useState<AnalyzerSource | null>(null)
  const [effect, setEffect] = useState<EffectView>(null)
  useEffect(() => {
    if (backend.kind !== "tauri") return
    let disposed = false
    void invoke<boolean>("analyzer_smoke_enabled").then((allowed) => {
      if (!disposed && allowed) setEnabled(true)
    }).catch(() => {})
    return () => { disposed = true }
  }, [])
  useEffect(() => {
    if (!enabled) return
    const control = (operation: string, value?: number) => invoke("analyzer_smoke_control", { operation, value: value ?? null })
    const api: SmokeApi = {
      async prepare() {
        const result = await invoke<Fixture>("analyzer_smoke_control", { operation:"prepare", value:null })
        setFixture(result)
        setSource({ type:"trackPostFader", track:result.tracks[0] })
        useUiStore.setState((state) => ({ mixerMeterMode:"waveform", panels:{ ...state.panels, mixer:true } }))
        return result
      },
      control,
      select(side) {
        if (!fixture) throw new Error("Prepare the native fixture first")
        setSource(side === "mix" ? { type:"mixOutput" } : { type:"trackPostFader", track:fixture.tracks[side === "left" ? 0 : 1] })
      },
      effect: setEffect,
      async deleteSource() {
        if (source?.type !== "trackPostFader") throw new Error("Select a track before deleting it")
        await backend.dispatch({ type:"removeMixerTrack", id:source.track })
      },
      read: () => readDisplays(document.querySelector("[data-slot=analyzer-smoke]")),
    }
    window.__windfallAnalyzerSmoke = api
    return () => { if (window.__windfallAnalyzerSmoke === api) delete window.__windfallAnalyzerSmoke }
  }, [enabled, fixture, source])
  return enabled ? <SmokeViews fixture={fixture} source={source} effect={effect} /> : null
}
