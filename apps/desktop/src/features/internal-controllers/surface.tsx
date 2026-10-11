import type { ParamBinder } from "@/features/params"
import { ParamControl } from "@/features/params"
import { ContextActions } from "@/components/context-actions"
import { Button } from "@/components/ui/button"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Slider } from "@/components/ui/slider"
import type { SurfaceControlKind } from "@/bindings"
import { useControllerConfig } from "./model"

export function SurfacePanel({ controller, bind, dashboard = false }: { controller: number; bind: ParamBinder; dashboard?: boolean }) {
  const { config, save } = useControllerConfig(controller)
  const controls = config.controls ?? []
  const id = (output: number) => dashboard ? `controllers.${output}.value` : `knob${output + 1}`
  const scale = dashboard ? 127 : 1
  return <div className="flex flex-col gap-3" aria-label={dashboard ? "Dashboard panel" : "Control surface panel"}>
    <div className="grid grid-cols-8 gap-2">{controls.map((control) => {
      const binding = bind(id(control.output))
      return <div key={control.output} style={{ gridColumn: control.column + 1, gridRow: control.row + 1 }} className="col-span-1 min-w-0">
        {control.kind === "knob" ? <ParamControl {...binding} info={{ ...binding.info, name: control.name }} label={control.name} /> : <ContextActions items={[...binding.contextItems]}>
          <div className="flex flex-col gap-2"><span className="text-xs">{control.name}</span>
            {control.kind === "button" ? <Button size="sm" variant="outline" aria-label={control.name} aria-pressed={binding.value >= scale / 2} disabled={binding.disabled} onClick={() => { binding.onGestureStart(); binding.onValueChange(binding.value >= scale / 2 ? 0 : scale); binding.onGestureEnd(); }}>{binding.value >= scale / 2 ? "On" : "Off"}</Button> : <Slider disabled={binding.disabled} aria-label={control.name} value={[binding.value]} min={0} max={scale} step={dashboard ? 1 : 0.001} onPointerDown={binding.onGestureStart} onPointerUp={binding.onGestureEnd} onPointerCancel={binding.onGestureEnd} onLostPointerCapture={binding.onGestureEnd} onBlur={binding.onGestureEnd} onValueCommitted={binding.onGestureEnd} onValueChange={(values) => binding.onValueChange(Array.isArray(values) ? values[0] : values)} />}
          </div>
        </ContextActions>}
      </div>
    })}</div>
    <FieldGroup>{controls.map((control) => <Field key={control.output}>
      <FieldLabel>Control {control.output + 1}</FieldLabel>
      <div className="flex flex-wrap gap-2">
        <Input aria-label={`Control ${control.output + 1} name`} defaultValue={control.name} key={control.name} maxLength={64} onBlur={(e) => { if (e.currentTarget.value.trim()) void save({ controls: controls.map((c) => c.output === control.output ? { ...c, name: e.currentTarget.value } : c) }); }} />
        <select aria-label={`Control ${control.output + 1} style`} value={control.kind} onChange={(e) => { const kind = e.currentTarget.value as SurfaceControlKind; void save({ controls: controls.map((c) => c.output === control.output ? { ...c, kind } : c) }); }}><option value="knob">Knob</option><option value="slider">Slider</option><option value="button">Button</option></select>
        {(["column", "row"] as const).map((field) => <Input key={field} aria-label={`Control ${control.output + 1} ${field}`} type="number" min={0} max={7} value={control[field]} onChange={(e) => { const value = Number(e.currentTarget.value); if (Number.isInteger(value) && value >= 0 && value <= 7) void save({ controls: controls.map((c) => c.output === control.output ? { ...c, [field]: value } : c) }); }} />)}
        <Button size="sm" variant="outline" onClick={() => { void save({ controls: controls.filter((c) => c.output !== control.output) }); }}>Delete control</Button>
      </div>
    </Field>)}</FieldGroup>
    <Button size="sm" variant="outline" disabled={controls.length >= 8} onClick={() => { const output = Array.from({ length: 8 }, (_, i) => i).find((i) => !controls.some((c) => c.output === i)); const cell = Array.from({ length: 64 }, (_, i) => ({ column: i % 8, row: Math.floor(i / 8) })).find((cell) => !controls.some((c) => c.column === cell.column && c.row === cell.row)); if (output !== undefined && cell) void save({ controls: [...controls, { output, name: `Control ${output + 1}`, kind: "knob", ...cell }] }); }}>Add control</Button>
    {controls.length === 0 && <p className="text-xs text-muted-foreground">Add a control, choose its appearance, then link its output from a destination control.</p>}
  </div>
}
