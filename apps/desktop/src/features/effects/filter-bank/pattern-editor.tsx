import { useEffect, useRef, type PointerEvent } from "react"
import { Button } from "@/components/ui/button"
import type { ParamBinder } from "@/features/params"
import { ParamFader } from "../controls/param-fader"

const POINTS = 8
/** A fixed-time curve: draw across it, or edit its points by keyboard/value entry. */
export function PatternEditor({ bind, unit, kind }: {
  bind: ParamBinder; unit: number; kind: "gate" | "cutoff"
}) {
  const prefix = `patterns.${unit}.${kind}`
  const group = bind.group(prefix)
  // Selecting another unit or closing the effect must finish a held stroke.
  useEffect(() => () => group.onGestureEnd(), [group])
  const held = useRef<{ index: number; value: number } | null>(null)
  const ids = Array.from({ length: POINTS }, (_, i) => `${prefix}.${i}`)
  const values = ids.map((id) => bind.value(id))
  const min = kind === "gate" ? 0 : -1
  const label = `Unit ${unit + 1} ${kind} envelope`
  const y = (value: number) => 90 - ((value - min) / (1 - min)) * 80
  const points = values.map((value, i) => `${10 + i * 40},${y(value)}`).join(" ")
  const draw = (event: PointerEvent<SVGSVGElement>) => {
    const box = event.currentTarget.getBoundingClientRect()
    if (!box.width || !box.height) return
    const index = Math.min(7, Math.max(0, Math.round(((event.clientX - box.left) / box.width * 320 - 10) / 40)))
    const fraction = Math.min(1, Math.max(0, (90 - (event.clientY - box.top) / box.height * 100) / 80))
    const value = min + fraction * (1 - min)
    const previous = held.current
    if (previous && previous.index !== index) {
      const step = index > previous.index ? 1 : -1
      for (let at = previous.index; at !== index; at += step) {
        const share = (at - previous.index) / (index - previous.index)
        group.set(ids[at], previous.value + share * (value - previous.value))
      }
    }
    group.set(ids[index], value)
    held.current = { index, value }
  }
  const finish = () => {
    if (held.current) group.onGestureEnd()
    held.current = null
  }
  const flatten = () => {
    group.onGestureStart()
    for (const id of ids) group.set(id, kind === "gate" ? 1 : 0)
    group.onGestureEnd()
  }
  return (
    <section aria-label={label} className="flex min-w-0 flex-col gap-1">
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs text-muted-foreground">{kind === "gate" ? "Volume gate" : "Cutoff pattern"}</span>
        <Button size="xs" variant="outline" disabled={bind.disabled} onClick={flatten}>
          {kind === "gate" ? "Open gate" : "Center cutoff"}
        </Button>
      </div>
      <svg role="img" aria-label={`${label}: ${values.map((v) => v.toFixed(2)).join(", ")}`}
        data-slot={`${kind}-pattern`} viewBox="0 0 320 100"
        className="h-24 w-full touch-none rounded-md border bg-chassis text-foreground"
        onPointerDown={(event) => {
          if (bind.disabled || event.button !== 0) return
          event.preventDefault()
          event.currentTarget.setPointerCapture(event.pointerId)
          group.onGestureStart()
          draw(event)
        }}
        onPointerMove={(event) => { if (held.current) draw(event) }}
        onPointerUp={finish} onPointerCancel={finish} onLostPointerCapture={finish}>
        <line x1="10" y1={y(kind === "gate" ? 0.5 : 0)} x2="310" y2={y(kind === "gate" ? 0.5 : 0)} stroke="currentColor" opacity="0.2" />
        <polyline points={`${points} 310,${y(values[0])}`} fill="none" stroke="currentColor" strokeWidth="2" />
        {values.map((value, i) => <circle key={i} cx={10 + i * 40} cy={y(value)} r="3" fill="currentColor" />)}
      </svg>
      <div className="grid grid-cols-8 gap-1">
        {ids.map((id, i) => <ParamFader key={id} binding={bind(id)} label={String(i + 1)} className="h-16" />)}
      </div>
    </section>
  )
}
