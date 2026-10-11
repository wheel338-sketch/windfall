import { useRef, useState, type PointerEvent } from "react"

import type { CurvePoint } from "@/bindings/CurvePoint"
import { Button } from "@/components/ui/button"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"

const LIMIT = 64
const clamp = (value: number) => Math.max(0, Math.min(1, value))

/** Local previews never publish assets. One release publishes one immutable curve. */
export function CurveEditor({
  label, points, onCommit, fallback = [{ x: 0, y: 0, tension: 0 }, { x: 1, y: 1, tension: 0 }],
}: {
  label: string
  points: readonly CurvePoint[]
  fallback?: readonly CurvePoint[]
  onCommit(points: CurvePoint[]): void
}) {
  const [draft, setDraft] = useState<CurvePoint[] | null>(null)
  const [selected, setSelected] = useState(0)
  const drag = useRef<{ pointer: number; index: number; points: CurvePoint[] } | null>(null)
  const shown = draft ?? (points.length ? points : fallback)
  const index = Math.min(selected, shown.length - 1)
  const point = shown[index]
  const sorted = [...shown].sort((a, b) => a.x - b.x)
  const commit = (next: readonly CurvePoint[]) => {
    const sorted = [...next].sort((a, b) => a.x - b.x)
    setSelected(Math.max(0, sorted.indexOf(next[Math.min(index, next.length - 1)])))
    onCommit(sorted)
    setDraft(null)
  }
  const coordinate = (event: PointerEvent<SVGSVGElement>) => {
    const rect = event.currentTarget.getBoundingClientRect()
    return { x: clamp((event.clientX - rect.left) / Math.max(1, rect.width)), y: clamp(1 - (event.clientY - rect.top) / Math.max(1, rect.height)) }
  }
  const cancel = () => { drag.current = null; setDraft(null) }
  return (
    <div role="group" aria-label={label} className="flex max-w-xl flex-col gap-2">
      <svg
        role="application" aria-label={`${label} drawing`} tabIndex={0}
        viewBox="0 0 100 100" preserveAspectRatio="none"
        className="h-48 w-full touch-none rounded border border-border bg-muted text-primary"
        onPointerDown={(event) => {
          if (event.button !== 0 || drag.current) return
          const at = coordinate(event)
          const nearest = shown.reduce((best, p, i) => Math.hypot(p.x - at.x, p.y - at.y) < Math.hypot(shown[best].x - at.x, shown[best].y - at.y) ? i : best, 0)
          const next = [...shown]
          let moved = nearest
          if (Math.hypot(shown[nearest].x - at.x, shown[nearest].y - at.y) > 0.08 && next.length < LIMIT) {
            moved = next.length
            next.push({ ...at, tension: 0 })
          } else next[moved] = { ...next[moved], ...at }
          setSelected(moved)
          drag.current = { pointer: event.pointerId, index: moved, points: next }
          setDraft(next)
          event.currentTarget.setPointerCapture(event.pointerId)
        }}
        onPointerMove={(event) => {
          const active = drag.current
          if (!active || active.pointer !== event.pointerId) return
          const next = [...active.points]
          next[active.index] = { ...next[active.index], ...coordinate(event) }
          active.points = next
          setDraft(next)
        }}
        onPointerUp={(event) => {
          const active = drag.current
          if (!active || active.pointer !== event.pointerId) return
          drag.current = null
          commit(active.points)
          event.currentTarget.releasePointerCapture(event.pointerId)
        }}
        onPointerCancel={cancel} onLostPointerCapture={cancel} onBlur={cancel}
        onKeyDown={(event) => {
          if (event.key === "Escape") { cancel(); return }
          if (event.key === "Delete" && shown.length > 2) { commit(shown.filter((_, i) => i !== index)); return }
          if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return
          event.preventDefault()
          const delta = event.shiftKey ? 0.1 : 0.01
          const next = [...shown]
          next[index] = { ...point, x: clamp(point.x + (event.key === "ArrowRight" ? delta : event.key === "ArrowLeft" ? -delta : 0)), y: clamp(point.y + (event.key === "ArrowUp" ? delta : event.key === "ArrowDown" ? -delta : 0)) }
          commit(next)
        }}
      >
        <path d="M 0 50 H 100 M 50 0 V 100" stroke="currentColor" opacity="0.2" strokeWidth="0.5" />
        <polyline fill="none" stroke="currentColor" strokeWidth="1" points={sorted.flatMap((from, i) => {
          const to = sorted[i + 1]
          if (!to) return [`${from.x * 100},${(1 - from.y) * 100}`]
          return Array.from({ length: 24 }, (_, n) => {
            const t = n / 24
            const y = from.y + (to.y - from.y) * Math.pow(t, Math.pow(4, from.tension))
            return `${(from.x + (to.x - from.x) * t) * 100},${(1 - y) * 100}`
          })
        }).join(" ")} />
        {shown.map((p, i) => <circle key={i} cx={p.x * 100} cy={(1 - p.y) * 100} r={i === index ? 2 : 1.3} fill="currentColor" />)}
      </svg>
      <FieldGroup className="flex flex-row gap-2">
        {(["x", "y", "tension"] as const).map((axis) => (
          <Field key={axis}>
            <FieldLabel htmlFor={`${label}-${axis}`}>{axis === "x" ? "Position" : axis === "y" ? "Value" : "Bend"}</FieldLabel>
            <Input id={`${label}-${axis}`} aria-label={`${label} ${axis}`} type="number" min={axis === "tension" ? -1 : 0} max={1} step={0.01} value={point?.[axis] ?? 0} onChange={(event) => {
              const value = Number(event.target.value)
              if (!Number.isFinite(value)) return
              const next = [...shown]
              next[index] = { ...point, [axis]: Math.max(axis === "tension" ? -1 : 0, Math.min(1, value)) }
              commit(next)
            }} />
          </Field>
        ))}
      </FieldGroup>
      <div className="flex items-center gap-2">
        <Button size="sm" variant="outline" onClick={() => commit(fallback)}>Reset {label}</Button>
        <Button size="sm" variant="outline" disabled={shown.length <= 2} onClick={() => commit(shown.filter((_, i) => i !== index))}>Delete point</Button>
        <span className="text-xs text-muted-foreground">{shown.length}/{LIMIT} points. Drag to add or move; arrows adjust; Escape cancels.</span>
      </div>
    </div>
  )
}
