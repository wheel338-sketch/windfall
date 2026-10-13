import { useRef, useState, type PointerEvent } from "react"
import type { PlaybackCurve } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

/** Draw a segment by inserting a point or dragging its nearest existing point.
 * One pointer gesture commits one asset edit. Numeric fields offer keyboard editing. */
export function CurveEditor({ curve, onChange }: { curve: PlaybackCurve; onChange(curve: PlaybackCurve): void }) {
  const [draft, setDraft] = useState<PlaybackCurve | null>(null)
  const drag = useRef<{ curve: PlaybackCurve; index: number } | null>(null)
  const visible = draft ?? curve
  const minimum = visible.mode === "speed" ? -8 : 0
  const span = visible.mode === "speed" ? 16 : 1
  const path = visible.points.map((p, i) => `${i ? "L" : "M"}${p.time * 300},${100 - ((p.value - minimum) / span) * 100}`).join(" ")
  const location = (event: PointerEvent<SVGSVGElement>) => {
    const box = event.currentTarget.getBoundingClientRect()
    return { time: Math.max(0, Math.min(1, (event.clientX - box.left) / Math.max(1, box.width))),
      value: minimum + span * Math.max(0, Math.min(1, 1 - (event.clientY - box.top) / Math.max(1, box.height))) }
  }
  const move = (event: PointerEvent<SVGSVGElement>) => {
    const run = drag.current
    if (!run) return
    const point = location(event)
    const points = [...run.curve.points]
    const index = run.index
    point.time = index === 0 ? 0 : index === points.length - 1 ? 1
      : Math.max(points[index - 1].time + 0.00001, Math.min(points[index + 1].time - 0.00001, point.time))
    points[index] = point
    run.curve = { ...run.curve, points }
    setDraft(run.curve)
  }
  return <FieldGroup>
    <Field><FieldLabel>{t("Curve mode")}</FieldLabel>
      <ToggleGroup value={[visible.mode]} onValueChange={(values) => {
        const mode = values[0]
        if (mode === "position" || mode === "speed") onChange({ ...curve, mode,
          points: curve.points.map((p) => ({ ...p, value: mode === "position" ? Math.max(0, Math.min(1, p.value)) : p.value })) })
      }}><ToggleGroupItem value="position">{t("Position")}</ToggleGroupItem><ToggleGroupItem value="speed">{t("Speed")}</ToggleGroupItem></ToggleGroup>
    </Field>
    <svg role="img" aria-label={t("Draw playback curve")} viewBox="0 0 300 100" className="h-32 w-full touch-none rounded border bg-muted"
      onPointerDown={(event) => {
        if (event.button !== 0) return
        const at = location(event)
        let index = curve.points.findIndex((p) => Math.abs(p.time - at.time) < 0.025)
        let points = [...curve.points]
        if (index < 0) {
          if (points.length >= 256) return
          points = [...points, at].sort((a, b) => a.time - b.time)
          index = points.indexOf(at)
        }
        drag.current = { curve: { ...curve, points }, index }
        event.currentTarget.setPointerCapture(event.pointerId)
        move(event)
      }} onPointerMove={move} onPointerCancel={() => { drag.current = null; setDraft(null) }}
      onPointerUp={(event) => {
        const run = drag.current
        if (!run) return
        drag.current = null; setDraft(null)
        event.currentTarget.releasePointerCapture(event.pointerId)
        onChange(run.curve)
      }}>
      <path d={path} fill="none" stroke="currentColor" strokeWidth="2" />
      {visible.points.map((p, i) => <circle key={i} cx={p.time * 300} cy={100 - ((p.value - minimum) / span) * 100} r="3" fill="currentColor" />)}
    </svg>
    {visible.points.map((point, index) => <Field key={`${index}:${point.time}:${point.value}`} orientation="horizontal">
      <FieldLabel>{t("Point")}{" "}{index + 1}</FieldLabel>
      <Input aria-label={message("Point {index} time", { index: plainNumber(index + 1) })} type="number" min={0} max={1} step={0.01} defaultValue={point.time}
        disabled={index === 0 || index === visible.points.length - 1} onBlur={(e) => {
          const time = Number(e.target.value)
          if (index > 0 && index < visible.points.length - 1 && Number.isFinite(time)
            && time > visible.points[index - 1].time && time < visible.points[index + 1].time
            && time !== point.time)
            onChange({ ...curve, points: curve.points.map((p, i) => i === index ? { ...p, time } : p) })
        }} />
      <Input aria-label={message("Point {index} value", { index: plainNumber(index + 1) })} type="number" min={minimum} max={minimum + span} step={0.01} defaultValue={point.value}
        onBlur={(e) => { const value = Number(e.target.value); if (Number.isFinite(value) && value >= minimum && value <= minimum + span && value !== point.value)
          onChange({ ...curve, points: curve.points.map((p, i) => i === index ? { ...p, value } : p) }) }} />
      <Button size="sm" variant="outline" disabled={index === 0 || index === visible.points.length - 1}
        onClick={() => onChange({ ...curve, points: curve.points.filter((_, i) => i !== index) })}>{t("Delete point")}{" "}{index + 1}</Button>
    </Field>)}
    {visible.mode === "speed" && <Field><FieldLabel>{t("Starting position")}</FieldLabel><Input aria-label={t("Starting position")} type="number" min={0} max={1} step={0.01}
      key={curve.origin} defaultValue={curve.origin} onBlur={(e) => { const origin = Number(e.target.value); if (Number.isFinite(origin) && origin >= 0 && origin <= 1 && origin !== curve.origin) onChange({ ...curve, origin }) }} /></Field>}
    <Button size="sm" variant="outline" disabled={curve.points.length >= 256} onClick={() => {
      const intervals = curve.points.slice(1).map((p, i) => ({ i, width: p.time - curve.points[i].time }))
      const { i } = intervals.reduce((a, b) => a.width >= b.width ? a : b)
      const a = curve.points[i]; const b = curve.points[i + 1]
      const points = [...curve.points]; points.splice(i + 1, 0, { time: (a.time + b.time) / 2, value: (a.value + b.value) / 2 })
      onChange({ ...curve, points })
    }}>{t("Add curve point")}</Button>
  </FieldGroup>
}
