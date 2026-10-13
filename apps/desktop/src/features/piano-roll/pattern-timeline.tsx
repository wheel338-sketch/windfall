import { useRef, useState } from "react"
import { create } from "zustand"
import type { PatternId, PatternTimelineEdit, TimeSignature, Timeline } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { dispatch, onHistoryNavigation, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { MAX_PATTERN_TICKS } from "@/lib/units"
import { meterSegments } from "@/lib/timeline"
import { seek, useTransportStore } from "@/lib/store/transport"
import { currentSession } from "./session"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const EMPTY: Timeline = { meters: [], markers: [] }
type Request = { pattern: PatternId; tick: number; generation: number }
const useTimelineDialog = create<{ request: Request | null }>(() => ({ request: null }))
export function closePatternTimeline() { useTimelineDialog.setState({ request: null }) }
onProjectReplaced(closePatternTimeline)
onHistoryNavigation(closePatternTimeline)
export function openPatternTimeline(tick = 0) {
  const pattern = currentSession()?.editor.context?.pattern.id
  if (pattern === undefined) return
  useTimelineDialog.setState({ request: { pattern, tick: Math.max(0, Math.min(MAX_PATTERN_TICKS, Math.round(tick))), generation: getProjectGeneration() } })
}

export function patternMeterAt(tick: number, signature: TimeSignature, timeline?: Timeline) {
  const segments = meterSegments(signature, timeline?.meters ?? [])
  return segments.findLast((segment) => segment.start <= tick) ?? segments[0]
}

function TimelineEditor({ request }: { request: Request }) {
  const pattern = useProjectStore((state) => state.project.patterns.find((item) => item.id === request.pattern))
  const inherited = useProjectStore((state) => state.project.settings.timeSignature)
  const [tick, setTick] = useState(String(request.tick))
  const [name, setName] = useState("Marker")
  const initial = patternMeterAt(request.tick, pattern?.timeSignature ?? inherited, pattern?.timeline).signature
  const [numerator, setNumerator] = useState(String(initial.numerator))
  const [denominator, setDenominator] = useState(String(initial.denominator))
  const [target, setTarget] = useState<{ kind: "marker" | "meter"; id: number } | null>(null)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const submitting = useRef(false)
  const timeline = pattern?.timeline ?? EMPTY
  if (!pattern || request.generation !== getProjectGeneration()) return null

  async function apply(edit: PatternTimelineEdit) {
    if (submitting.current || useTimelineDialog.getState().request !== request || request.generation !== getProjectGeneration()) return
    const source = useProjectStore.getState().project.patterns.find((item) => item.id === request.pattern)
    if (!source) return
    submitting.current = true
    setPending(true)
    setError(null)
    try {
      const result = await dispatch({ type: "editPatternTimeline", pattern: source.id, expected: source.timeline ?? EMPTY, expectedSignature: source.timeSignature ?? null, edit })
      if (useTimelineDialog.getState().request !== request) return
      if (result) setTarget(null)
      else setError(t("The edit could not be applied. Meter changes need distinct ticks and markers need a name of up to 256 bytes."))
    } finally { submitting.current = false; setPending(false) }
  }
  function signature(): TimeSignature | null {
    const result = { numerator: Number(numerator), denominator: Number(denominator) }
    if (!Number.isInteger(result.numerator) || result.numerator < 1 || result.numerator > 16 || ![2, 4, 8, 16].includes(result.denominator)) {
      setError(t("Use 1–16 beats with a beat unit of 2, 4, 8 or 16."))
      return null
    }
    return result
  }
  function at(): number | null {
    const result = Number(tick)
    if (!tick.trim() || !Number.isInteger(result) || result < 0 || result > MAX_PATTERN_TICKS) { setError(message("Enter a whole tick from 0 to {maxPatternTicks}.", { maxPatternTicks: plainNumber(MAX_PATTERN_TICKS) })); return null }
    return result
  }
  return <Dialog open onOpenChange={(open) => { if (!open && !pending) closePatternTimeline() }}>
    <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-xl">
      <DialogHeader><DialogTitle>{pattern.name}{t(": markers and time signatures")}</DialogTitle><DialogDescription>{t("Pattern timing labels and meters apply to this pattern. Playlist playback follows the song timeline.")}</DialogDescription></DialogHeader>
      <fieldset disabled={pending} className="space-y-4">
        <p className="text-sm">{t("Base signature:")}{" "}{pattern.timeSignature ? `${pattern.timeSignature.numerator}/${pattern.timeSignature.denominator}` : message("Song ({numerator}/{denominator})", { numerator: plainNumber(inherited.numerator), denominator: plainNumber(inherited.denominator) })}</p>
        <div className="flex flex-wrap items-end gap-2">
          <label className="space-y-1 text-xs">{t("Beats")}<Input type="number" min={1} max={16} value={numerator} onChange={(event) => setNumerator(event.target.value)} /></label>
          <label className="space-y-1 text-xs">{t("Beat unit")}<Input type="number" value={denominator} onChange={(event) => setDenominator(event.target.value)} /></label>
          <Button size="sm" onClick={() => { const value = signature(); if (value) void apply({ type: "setSignature", signature: value }) }}>{t("Set base")}</Button>
          <Button size="sm" variant="outline" onClick={() => void apply({ type: "setSignature", signature: null })}>{t("Use song signature")}</Button>
        </div>
        <div className="flex gap-2">
          <label className="space-y-1 text-xs">{t("Pattern tick")}<Input type="number" min={0} max={MAX_PATTERN_TICKS} value={tick} onChange={(event) => setTick(event.target.value)} /></label>
          <label className="min-w-0 flex-1 space-y-1 text-xs">{t("Marker name")}<Input value={name} onChange={(event) => setName(event.target.value)} /></label>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" onClick={() => {
            const position = at(); if (position === null) return
            if (!name.trim() || new TextEncoder().encode(name.trim()).length > 256) { setError(t("Enter a marker name of up to 256 bytes.")); return }
            void apply(target?.kind === "marker" ? { type: "updateMarker", marker: { id: target.id, tick: position, name, kind: { type: "named" } } } : { type: "addMarker", tick: position, name })
          }}>{target?.kind === "marker" ? t("Update marker") : t("Add marker")}</Button>
          <Button size="sm" variant="outline" onClick={() => {
            const position = at(); const value = signature(); if (position === null || !value) return
            void apply(target?.kind === "meter" ? { type: "updateMeter", change: { id: target.id, tick: position, signature: value } } : { type: "addMeter", tick: position, signature: value })
          }}>{target?.kind === "meter" ? t("Update meter") : t("Add meter change")}</Button>
          {target && <Button size="sm" variant="ghost" onClick={() => setTarget(null)}>{t("New item")}</Button>}
        </div>
        <div className="space-y-2 text-sm">
          <p className="font-medium">{t("Meter changes")}</p>
          {timeline.meters.length === 0 && <p className="text-muted-foreground">{t("No local meter changes")}</p>}
          {timeline.meters.map((meter) => <div key={meter.id} className="flex items-center gap-2"><span className="flex-1">{t("Tick")}{" "}{meter.tick} · {meter.signature.numerator}/{meter.signature.denominator}</span><Button size="sm" variant="ghost" onClick={() => { setTarget({ kind: "meter", id: meter.id }); setTick(String(meter.tick)); setNumerator(String(meter.signature.numerator)); setDenominator(String(meter.signature.denominator)) }}>{t("Edit")}</Button><Button size="sm" variant="ghost" onClick={() => void apply({ type: "removeMeter", id: meter.id })}>{t("Remove")}</Button></div>)}
          <p className="font-medium">{t("Markers")}</p>
          {timeline.markers.length === 0 && <p className="text-muted-foreground">{t("No pattern markers")}</p>}
          {timeline.markers.map((marker) => <div key={marker.id} className="flex items-center gap-2"><span className="min-w-0 flex-1 truncate">{t("Tick")}{" "}{marker.tick} · {marker.name}</span><Button size="sm" variant="ghost" disabled={useTransportStore.getState().mode !== "pattern" || useTransportStore.getState().pattern !== pattern.id} onClick={() => void seek(Math.min(marker.tick, pattern.lengthSteps * 240 - 1))}>{t("Go")}</Button><Button size="sm" variant="ghost" onClick={() => { setTarget({ kind: "marker", id: marker.id }); setTick(String(marker.tick)); setName(marker.name) }}>{t("Edit")}</Button><Button size="sm" variant="ghost" onClick={() => void apply({ type: "removeMarker", id: marker.id })}>{t("Remove")}</Button></div>)}
        </div>
      </fieldset>
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      <Button variant="outline" disabled={pending} onClick={closePatternTimeline}>{t("Close")}</Button>
    </DialogContent>
  </Dialog>
}

export function PatternTimelineDialog() {
  const request = useTimelineDialog((state) => state.request)
  return request ? <TimelineEditor key={`${request.generation}:${request.pattern}:${request.tick}`} request={request} /> : null
}
