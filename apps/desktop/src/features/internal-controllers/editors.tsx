import { useRef, useState, type PointerEvent } from "react"
import type { ControlPoint } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { ParamControl, type EffectParamsOf } from "@/features/params"
import type { EditorProps } from "@/features/effects/editor-props"
import { useProjectStore } from "@/lib/store/project"
import { newGestureId } from "@/lib/store/gesture"
import { useControllerConfig } from "./model"
import { SourceLinks } from "./links"
import { ControlPad } from "./pad"
import { SurfacePanel } from "./surface"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

function NoteInput({ controller }: { controller: number }) {
  const channels = useProjectStore((s) => s.project.channels)
  const { config, save } = useControllerConfig(controller)
  return <Field><FieldLabel>{t("Note input")}</FieldLabel><select aria-label={t("Note input")} value={config.noteChannel ?? "track"} onChange={(e) => { void save({ noteChannel: e.currentTarget.value === "track" ? undefined : Number(e.currentTarget.value) }); }}><option value="track">{t("Channels on this mixer track")}</option>{channels.map((c) => <option key={c.id} value={c.id}>{c.name || message("Channel {cId}", { cId: plainNumber(c.id) })}</option>)}</select></Field>
}
function DrawnEnvelope({ controller }: { controller: number }) {
  const { config, save } = useControllerConfig(controller)
  const [drawing, setDrawing] = useState<ControlPoint[] | null>(null)
  const drag = useRef<{ pointer: number; gesture: number; points: ControlPoint[] } | null>(null)
  const points = drawing ?? config.envelope ?? []
  function paint(event: PointerEvent<SVGSVGElement>) {
    const run = drag.current; if (!run) return
    const box = event.currentTarget.getBoundingClientRect(); if (!box.width || !box.height) return
    const position = Math.round(Math.max(0, Math.min(1, (event.clientX - box.left) / box.width)) * 16) / 16
    const value = Math.max(0, Math.min(1, 1 - (event.clientY - box.top) / box.height))
    const next = run.points.filter((p) => p.position !== position); next.push({ position, value }); next.sort((a, b) => a.position - b.position)
    run.points = next; setDrawing(next)
  }
  function finish(event: PointerEvent<SVGSVGElement>) {
    const run = drag.current; if (!run || run.pointer !== event.pointerId) return
    drag.current = null
    void Promise.resolve(save({ envelope: run.points, durationMs: config.durationMs || 1000 }, run.gesture)).finally(() => setDrawing(null))
  }
  return <FieldGroup>
    <svg role="img" aria-label={t("Drawn note envelope")} viewBox="0 0 320 120" className="h-32 w-full touch-none rounded-md border bg-muted" onPointerDown={(e) => { if (e.button !== 0 || drag.current) return; e.currentTarget.setPointerCapture(e.pointerId); drag.current = { pointer: e.pointerId, gesture: newGestureId(), points: config.envelope?.length ? [...config.envelope] : [{ position: 0, value: 0 }, { position: 1, value: 0 }] }; paint(e); }} onPointerMove={(e) => { if (drag.current?.pointer === e.pointerId) paint(e); }} onPointerUp={finish} onPointerCancel={finish} onLostPointerCapture={finish}>
      <polyline fill="none" stroke="currentColor" strokeWidth="2" points={points.map((p) => `${p.position * 320},${(1 - p.value) * 120}`).join(" ")} />
      {points.map((p) => <circle key={p.position} cx={p.position * 320} cy={(1 - p.value) * 120} r="3" fill="currentColor" />)}
    </svg>
    <Field><FieldLabel>{t("Envelope duration (ms)")}</FieldLabel><Input aria-label={t("Envelope duration")} type="number" min={1} max={60000} defaultValue={config.durationMs || 1000} key={config.durationMs} onBlur={(e) => { void save({ durationMs: Number(e.currentTarget.value) }); }} /></Field>
    {points.map((point, index) => <Field key={point.position}><FieldLabel>{t("Point at")}{" "}{Math.round(point.position * 100)}%</FieldLabel><Input aria-label={message("Envelope point {index}", { index: plainNumber(index + 1) })} type="number" min={0} max={1} step={0.01} defaultValue={point.value} key={point.value} onBlur={(e) => { void save({ envelope: points.map((p, at) => at === index ? { ...p, value: Number(e.currentTarget.value) } : p), durationMs: config.durationMs || 1000 }); }} /></Field>)}
    <Button size="sm" variant="outline" onClick={() => { void save({ envelope: [] }); }}>{t("Use ADSR envelope")}</Button>
  </FieldGroup>
}
export function EnvelopeSourceEditor({ effect, bind }: EditorProps<EffectParamsOf<"noteEnvelope">>) {
  return <div className="flex flex-col gap-3" aria-label={t("Note envelope source")}><NoteInput controller={effect} /><DrawnEnvelope controller={effect} /><div className="flex flex-wrap gap-2">{["attackMs", "decayMs", "sustain", "releaseMs", "trigger"].map((id) => <ParamControl key={id} {...bind(id)} />)}</div><SourceLinks controller={effect} /></div>
}
export function KeyboardSourceEditor({ effect, bind }: EditorProps<EffectParamsOf<"keyboardSource">>) {
  return <div className="flex flex-col gap-3" aria-label={t("Keyboard control source")}><NoteInput controller={effect} /><p className="text-xs text-muted-foreground">{t("Pitch uses MIDI key / 127; velocity uses 0-1. The last held note wins, then returns to the previous held note.")}</p><div className="flex flex-wrap gap-2">{["rootKey", "pitchAmount", "transpose", "velocityAmount"].map((id) => <ParamControl key={id} {...bind(id)} />)}</div><SourceLinks controller={effect} /></div>
}
export function FormulaSourceEditor({ effect, bind }: EditorProps<EffectParamsOf<"formulaSource">>) {
  const { config, save } = useControllerConfig(effect)
  const [expression, setExpression] = useState(config.expression ?? "a * b + c")
  const [error, setError] = useState<string | null>(null)
  return <div className="flex flex-col gap-3" aria-label={t("Formula control source")}><FieldGroup><Field data-invalid={error !== null}><FieldLabel>{t("Formula")}</FieldLabel><Input aria-label={t("Formula")} aria-invalid={error !== null} maxLength={256} value={expression} onChange={(e) => setExpression(e.currentTarget.value)} /><p className="text-xs text-muted-foreground">{t("Installed:")}{" "}{config.expression ?? "a * b + c"}</p><p className="text-xs text-muted-foreground">{t("a, b, c; + - * / ^; sin, abs, min, max. Output is bounded to 0-1.")}</p></Field><Button size="sm" onClick={() => { setError(null); void Promise.resolve(save({ expression })).then((result) => { if (!result) setError(t("Formula was refused. Check its syntax and bounds.")); }); }}>{t("Apply formula")}</Button></FieldGroup>{error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}<div className="flex gap-2">{["a", "b", "c", "base", "amount"].map((id) => <ParamControl key={id} {...bind(id)} />)}</div><SourceLinks controller={effect} /></div>
}
export function PeakSourceEditor({ effect, bind }: EditorProps<EffectParamsOf<"envelopeFollower">>) {
  const { config, save } = useControllerConfig(effect)
  return <div className="flex flex-col gap-3" aria-label={t("Audio peak and LFO source")}><p className="text-xs text-muted-foreground">{t("Detects real audio at this mixer slot before the fader. The LFO has a separate output.")}</p><FieldGroup><Field><FieldLabel><input type="checkbox" aria-label={t("RMS detector")} checked={config.rms} onChange={(e) => { void save({ rms: e.currentTarget.checked }); }} />{" "}{t("RMS detector")}</FieldLabel></Field><Field><FieldLabel><input type="checkbox" aria-label={t("Sync LFO to tempo")} checked={config.syncBeats !== undefined} onChange={(e) => { void save({ syncBeats: e.currentTarget.checked ? 1 : undefined }); }} />{" "}{t("Sync LFO to tempo")}</FieldLabel></Field>{config.syncBeats !== undefined && <Field><FieldLabel>{t("Beats per cycle")}</FieldLabel><Input aria-label={t("Beats per cycle")} type="number" min={0.0625} max={64} step={0.0625} defaultValue={config.syncBeats} key={config.syncBeats} onBlur={(e) => { void save({ syncBeats: Number(e.currentTarget.value) }); }} /></Field>}</FieldGroup><div className="flex flex-wrap gap-2">{["attackMs", "releaseMs", "inputGain", "base", "amount", "lfoDepth", "rateHz"].map((id) => <ParamControl key={id} {...bind(id)} />)}</div><SourceLinks controller={effect} /></div>
}
export function XySourceEditor({ effect, bind }: EditorProps<EffectParamsOf<"xyPad">>) { return <div className="flex flex-col gap-3"><ControlPad bind={bind} /><SourceLinks controller={effect} /></div> }
export function XyzSourceEditor({ effect, bind }: EditorProps<EffectParamsOf<"xyzPad">>) { return <div className="flex flex-col gap-3"><ControlPad bind={bind} xyz /><SourceLinks controller={effect} /></div> }
export function SurfaceSourceEditor({ effect, bind }: EditorProps<EffectParamsOf<"controlSurface">>) { return <div className="flex flex-col gap-3"><SurfacePanel controller={effect} bind={bind} /><SourceLinks controller={effect} /></div> }
export function VoltageSourceEditor({ effect, bind }: EditorProps<EffectParamsOf<"voltageSource">>) {
  const { config, save } = useControllerConfig(effect)
  const routes = config.voltageRoutes ?? []
  return <div className="flex flex-col gap-3" aria-label={t("DC control source")}><Alert><AlertDescription>{t("Software DC samples require a DC-coupled interface. Calibration is in audio sample units; hardware voltage and voltage safety must be verified externally. DC output starts disabled.")}</AlertDescription></Alert><ParamControl {...bind("armed")} /><div className="flex flex-wrap gap-2">{Array.from({ length: 8 }, (_, i) => <ParamControl key={i} {...bind(`value${i + 1}`)} />)}</div><FieldGroup>{routes.map((route) => <Field key={route.output}><FieldLabel>{t("Output")}{" "}{route.output + 1}{" "}{t("calibration")}</FieldLabel><div className="grid grid-cols-3 gap-2">{(["channel", "zero", "scale"] as const).map((field) => <Input key={field} aria-label={message("Output {output} {field}", { output: plainNumber(route.output + 1), field: t(field) })} type="number" step={field === "channel" ? 1 : 0.001} defaultValue={route[field]} onBlur={(e) => { void save({ voltageRoutes: routes.map((r) => r.output === route.output ? { ...r, [field]: Number(e.currentTarget.value) } : r) }); }} />)}</div><p className="text-xs text-muted-foreground">{t("Audio channel (0-based), zero sample, full-range sample delta")}</p><Button size="sm" variant="outline" onClick={() => { void save({ voltageRoutes: routes.filter((r) => r.output !== route.output) }); }}>{t("Remove output route")}</Button></Field>)}</FieldGroup><Button size="sm" variant="outline" disabled={routes.length >= 8} onClick={() => { const output = Array.from({ length: 8 }, (_, i) => i).find((i) => !routes.some((r) => r.output === i)); if (output !== undefined) void save({ voltageRoutes: [...routes, { output, channel: output + 2, zero: 0, scale: 1 }] }); }}>{t("Add output route")}</Button><SourceLinks controller={effect} /></div>
}
