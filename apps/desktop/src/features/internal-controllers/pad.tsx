import { useEffect, useRef, type PointerEvent } from "react"
import type { ParamBinder } from "@/features/params"
import { ParamControl } from "@/features/params"
import { ContextActions } from "@/components/context-actions"

/** The two axes share a parameter gesture and can be recorded by the host. */
export function ControlPad({ bind, xyz = false }: { bind: ParamBinder; xyz?: boolean }) {
  const held = useRef<number | null>(null)
  const group = bind.group("pad")
  useEffect(() => () => { if (held.current !== null) { held.current = null; group.onGestureEnd(); } }, [group])
  function position(event: PointerEvent<HTMLDivElement>) {
    const bounds = event.currentTarget.getBoundingClientRect()
    if (!bounds.width || !bounds.height) return
    group.set("x", Math.max(0, Math.min(1, (event.clientX - bounds.left) / bounds.width)))
    group.set("y", Math.max(0, Math.min(1, 1 - (event.clientY - bounds.top) / bounds.height)))
  }
  function finish(event: PointerEvent<HTMLDivElement>) {
    if (held.current !== event.pointerId) return
    held.current = null; group.onGestureEnd()
  }
  return <div className="flex flex-col gap-2">
    <ContextActions items={[...bind("x").contextItems, ...bind("y").contextItems]}>
      <div role="group" aria-label={xyz ? "XYZ control pad" : "XY control pad"} tabIndex={0} className="relative h-44 touch-none rounded-md border bg-muted"
        onPointerDown={(e) => { if (bind.disabled || held.current !== null) return; held.current = e.pointerId; e.currentTarget.setPointerCapture(e.pointerId); group.onGestureStart(); position(e); }}
        onPointerMove={(e) => { if (held.current === e.pointerId) position(e); }} onPointerUp={finish} onPointerCancel={finish} onLostPointerCapture={finish}
        onKeyDown={(e) => { const axis = e.key === "ArrowLeft" || e.key === "ArrowRight" ? "x" : e.key === "ArrowUp" || e.key === "ArrowDown" ? "y" : null; if (!axis || bind.disabled) return; e.preventDefault(); const delta = e.key === "ArrowLeft" || e.key === "ArrowDown" ? -0.01 : 0.01; group.onGestureStart(); group.set(axis, bind.value(axis) + delta); group.onGestureEnd(); }}>
        <span aria-hidden="true" className="pointer-events-none absolute size-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-primary" style={{ left: `${100 * bind.value("x")}%`, top: `${100 * (1 - bind.value("y"))}%` }} />
      </div>
    </ContextActions>
    <div className="flex gap-3"><ParamControl {...bind("x")} label="X" /><ParamControl {...bind("y")} label="Y" />{xyz && <ParamControl {...bind("z")} label="Z / depth" />}</div>
  </div>
}
