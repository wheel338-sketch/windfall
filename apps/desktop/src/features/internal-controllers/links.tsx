import type { ControlMap, ControllerLink } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { sameShape } from "@/features/controllers/menu"
import { targetTitle } from "./model"

/** Source-owned link editor; ranges are always in the target's automation travel. */
export function SourceLinks({ controller }: { controller: number }) {
  const project = useProjectStore((s) => s.project)
  const links = project.settings.controllerLinks ?? []
  const found = links.filter((link) => link.source.type === "internal" && link.source.controller === controller)
  function matches(link: ControllerLink, selected: ControllerLink) {
    return sameShape(link.source, selected.source) && sameShape(link.target, selected.target)
  }
  function change(selected: ControllerLink, map: Partial<ControlMap>) {
    const latest = useProjectStore.getState().project.settings.controllerLinks ?? []
    if (!latest.some((link) => matches(link, selected))) return
    void dispatch({ type: "setControllerLinks", links: latest.map((link) => matches(link, selected) ? { ...link, map: { ...link.map, ...map } } : link) })
  }
  return <section aria-label="Controller links" className="flex flex-col gap-2">
    <p className="text-xs text-muted-foreground">Right-click a destination control and choose Link to internal controller.</p>
    {found.map((link) => <FieldGroup key={JSON.stringify([link.source, link.target])}>
      <p className="text-xs">{link.target.type === "param" ? targetTitle(project, link.target.target) : link.target.type} · output {link.source.type === "internal" ? link.source.output + 1 : ""}</p>
      <div className="grid grid-cols-3 gap-2">
        {(["min", "max", "smoothingMs"] as const).map((field) => <Field key={field}>
          <FieldLabel>{field === "smoothingMs" ? "Smoothing (ms)" : field === "min" ? "Range start" : "Range end"}</FieldLabel>
          <Input aria-label={field} type="number" min={0} max={field === "smoothingMs" ? 5000 : 1} step={field === "smoothingMs" ? 1 : 0.01} defaultValue={link.map[field]} key={link.map[field]} onBlur={(event) => { const value = Number(event.currentTarget.value); if (Number.isFinite(value)) change(link, { [field]: value }); }} />
        </Field>)}
      </div>
      <Field><FieldLabel>Mapping formula (x)</FieldLabel><Input aria-label="Mapping formula" defaultValue={link.map.formula ?? ""} key={link.map.formula ?? ""} onBlur={(e) => change(link, { formula: e.currentTarget.value || undefined })} /></Field>
      <Button size="sm" variant="outline" onClick={() => { const latest = useProjectStore.getState().project.settings.controllerLinks ?? []; if (!latest.some((current) => matches(current, link))) return; void dispatch({ type: "setControllerLinks", links: latest.filter((current) => !matches(current, link)) }); }}>Remove link</Button>
    </FieldGroup>)}
  </section>
}
