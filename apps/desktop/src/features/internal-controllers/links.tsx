import type { ControlMap } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { targetTitle } from "./model"

/** Source-owned link editor; ranges are always in the target's automation travel. */
export function SourceLinks({ controller }: { controller: number }) {
  const project = useProjectStore((s) => s.project)
  const links = project.settings.controllerLinks ?? []
  const found = links.flatMap((link, index) => link.source.type === "internal" && link.source.controller === controller ? [{ link, index }] : [])
  function change(index: number, map: Partial<ControlMap>) {
    const latest = useProjectStore.getState().project.settings.controllerLinks ?? []
    void dispatch({ type: "setControllerLinks", links: latest.map((link, at) => at === index ? { ...link, map: { ...link.map, ...map } } : link) })
  }
  return <section aria-label="Controller links" className="flex flex-col gap-2">
    <p className="text-xs text-muted-foreground">Right-click a destination control and choose Link to internal controller.</p>
    {found.map(({ link, index }) => <FieldGroup key={index}>
      <p className="text-xs">{link.target.type === "param" ? targetTitle(project, link.target.target) : link.target.type} · output {link.source.type === "internal" ? link.source.output + 1 : ""}</p>
      <div className="grid grid-cols-3 gap-2">
        {(["min", "max", "smoothingMs"] as const).map((field) => <Field key={field}>
          <FieldLabel>{field === "smoothingMs" ? "Smoothing (ms)" : field === "min" ? "Range start" : "Range end"}</FieldLabel>
          <Input aria-label={field} type="number" min={0} max={field === "smoothingMs" ? 10000 : 1} step={field === "smoothingMs" ? 1 : 0.01} defaultValue={link.map[field]} key={link.map[field]} onBlur={(event) => { const value = Number(event.currentTarget.value); if (Number.isFinite(value)) change(index, { [field]: value }); }} />
        </Field>)}
      </div>
      <Field><FieldLabel>Mapping formula (x)</FieldLabel><Input aria-label="Mapping formula" defaultValue={link.map.formula ?? ""} key={link.map.formula ?? ""} onBlur={(e) => change(index, { formula: e.currentTarget.value || undefined })} /></Field>
      <Button size="sm" variant="outline" onClick={() => { const latest = useProjectStore.getState().project.settings.controllerLinks ?? []; void dispatch({ type: "setControllerLinks", links: latest.filter((_, at) => at !== index) }); }}>Remove link</Button>
    </FieldGroup>)}
  </section>
}
