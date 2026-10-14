import { useId, useState } from "react"
import type { EnvelopeDestination, NoteProperty, RackNoteModule, SequenceMode } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

export function NumberField({ label, value, min, max, step = 1, onChange }: {
  label: string; value: number; min: number; max: number; step?: number; onChange: (value: number) => void
}) {
  const id = useId()
  return <Field><FieldLabel htmlFor={id}>{label}</FieldLabel>
    <Input key={value} id={id} type="number" defaultValue={value} min={min} max={max} step={step}
      onBlur={(event) => { const next = Number(event.target.value); if (Number.isFinite(next) && next !== value) onChange(Math.min(max, Math.max(min, step === 1 ? Math.round(next) : next))) }}
      onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur() }} />
  </Field>
}

export function ChoiceField<T extends string>({ label, value, choices, onChange }: {
  label: string; value: T; choices: readonly T[]; onChange: (value: T) => void
}) {
  return <Field><FieldLabel>{label}</FieldLabel><Select value={value} onValueChange={(next) => { if (next !== null) onChange(next) }}>
    <SelectTrigger aria-label={label}><SelectValue /></SelectTrigger>
    <SelectContent><SelectGroup>{choices.map((choice) => <SelectItem key={choice} value={choice}>{choice}</SelectItem>)}</SelectGroup></SelectContent>
  </Select></Field>
}

export function NoteModuleEditor({ module, onChange }: { module: RackNoteModule; onChange: (module: RackNoteModule) => void }) {
  const [octave, setOctave] = useState(4)
  switch (module.type) {
    case "colorRouter": return <FieldGroup><p>{t("Connect a route output to the instrument that receives that color.")}</p>
      <div className="grid grid-cols-4 gap-2">{module.params.table.map((route, color) => <NumberField key={color} label={message("Color {color} route", { color: plainNumber(color + 1) })} value={route + 1} min={1} max={16} onChange={(value) => {
        const table = [...module.params.table]; table[color] = value - 1; onChange({ ...module, params: { table } })
      }} />)}</div></FieldGroup>
    case "keySplitter": return <FieldGroup><Field orientation="horizontal"><Checkbox id="preserve-rack-key" checked={module.preserveKey} onCheckedChange={(value) => onChange({ ...module, preserveKey: value === true })} />
      <FieldLabel htmlFor="preserve-rack-key">{t("Keep incoming pitch")}</FieldLabel></Field>
      <div className="grid grid-cols-2 gap-2">{(["lowMin", "boundary", "highMax", "lowKey", "highKey", "lowChannel", "highChannel"] as const).map((key) =>
        <NumberField key={key} label={t(key)} value={module.params[key]} min={0} max={key.endsWith("Channel") ? 15 : 127} onChange={(value) => onChange({ ...module, params: { ...module.params, [key]: value } })} />)}</div>
      <p>{t("Low keys include the minimum and stop below the boundary. High keys include the boundary and maximum.")}</p>
    </FieldGroup>
    case "keyMapper": return <FieldGroup><NumberField label={t("Key map octave")} value={octave} min={0} max={10} onChange={setOctave} />
      <div className="grid grid-cols-4 gap-2">{Array.from({ length: 12 }, (_, offset) => octave * 12 + offset).filter((key) => key < 128).map((key) =>
        <NumberField key={key} label={message("Key {key} maps to", { key: plainNumber(key) })} value={module.params.table[key]} min={0} max={127} onChange={(value) => { const table = [...module.params.table]; table[key] = value; onChange({ ...module, params: { table } }) }} />)}</div>
      <NumberField label={t("Chord voices")} value={module.voices} min={1} max={4} onChange={(voices) => onChange({ ...module, voices })} />
      <div className="grid grid-cols-4 gap-2">{module.chord.map((value, index) => <NumberField key={index} label={message("Chord offset {index}", { index: plainNumber(index + 1) })} value={value} min={-127} max={127} onChange={(value) => {
        const chord = [...module.chord]; chord[index] = value; onChange({ ...module, chord })
      }} />)}</div>
      <Button variant="outline" onClick={() => onChange({ ...module, params: { table: Array.from({ length: 128 }, (_, key) => key) } })}>{t("Reset key map")}</Button>
    </FieldGroup>
    case "propertyScaler": return <FieldGroup>
      <ChoiceField<NoteProperty> label={t("Note property")} value={module.params.property} choices={["velocity", "pan", "release", "pitch", "modulationX", "modulationY"]} onChange={(property) => onChange({ ...module, params: { ...module.params, property } })} />
      <svg role="img" aria-label={t("Property mapping curve")} viewBox="0 0 200 100" className="h-24 w-full text-primary"><polyline fill="none" stroke="currentColor" points={Array.from({ length: 41 }, (_, index) => {
        const x = index / 40; const y = Math.min(Math.max(module.params.levels.ceiling, module.params.levels.floor), Math.max(module.params.levels.floor, x ** module.params.curve * module.params.levels.percent / 100)); return `${x * 200},${100 - y * 100}`
      }).join(" ")} /></svg>
      <NumberField label={t("Curve")} value={module.params.curve} min={0.125} max={8} step={0.125} onChange={(curve) => onChange({ ...module, params: { ...module.params, curve } })} />
      {(["percent", "floor", "ceiling"] as const).map((key) => <NumberField key={key} label={t(key)} value={module.params.levels[key]} min={0} max={key === "percent" ? 200 : 1} step={key === "percent" ? 1 : 0.01} onChange={(value) => onChange({ ...module, params: { ...module.params, levels: { ...module.params.levels, [key]: value } } })} />)}
    </FieldGroup>
    case "envelope": return <FieldGroup>
      <ChoiceField<EnvelopeDestination> label={t("Envelope destination")} value={module.params.destination} choices={["gain", "pan", "pitch", "modulationX", "modulationY"]} onChange={(destination) => onChange({ ...module, params: { ...module.params, destination } })} />
      <svg role="img" aria-label={t("Note envelope")} viewBox="0 0 200 100" className="h-24 w-full text-primary"><polyline fill="none" stroke="currentColor" points={`0,100 35,0 80,${100 - module.params.sustain * 100} 140,${100 - module.params.sustain * 100} 200,100`} /></svg>
      {(["attackMs", "decayMs", "sustain", "releaseMs", "amount"] as const).map((key) => <NumberField key={key} label={t(key)} value={module.params[key]} min={key === "amount" ? -1 : 0} max={key.endsWith("Ms") ? 10000 : 1} step={key.endsWith("Ms") ? 1 : 0.01} onChange={(value) => onChange({ ...module, params: { ...module.params, [key]: value } })} />)}
    </FieldGroup>
    case "sequencer": return <FieldGroup>
      <ChoiceField<SequenceMode> label={t("Sequence mode")} value={module.params.mode} choices={["sequence", "up", "down", "asPlayed"]} onChange={(mode) => onChange({ ...module, params: { ...module.params, mode } })} />
      <div className="grid grid-cols-4 gap-2">
      <NumberField label={t("Sequence length")} value={module.params.length} min={1} max={16} onChange={(length) => onChange({ ...module, params: { ...module.params, length } })} />
      <NumberField label={t("Steps per beat")} value={module.params.grid.stepsPerBeat} min={1} max={16} onChange={(stepsPerBeat) => onChange({ ...module, params: { ...module.params, grid: { ...module.params.grid, stepsPerBeat } } })} />
      <NumberField label={t("Gate length")} value={module.params.gate} min={0.01} max={1} step={0.01} onChange={(gate) => onChange({ ...module, params: { ...module.params, gate } })} />
      <NumberField label={t("Arpeggio octaves")} value={module.params.octaves} min={1} max={4} onChange={(octaves) => onChange({ ...module, params: { ...module.params, octaves } })} />
      </div><div className="grid grid-cols-4 gap-3">{module.params.grid.steps.map((step, index) => {
        const update = (next: typeof step) => { const steps = [...module.params.grid.steps]; steps[index] = next; onChange({ ...module, params: { ...module.params, grid: { ...module.params.grid, steps } } }) }
        return <FieldGroup key={index}><Field orientation="horizontal"><Checkbox id={`rack-step-${index}`} checked={step.gate} onCheckedChange={(gate) => update({ ...step, gate: gate === true })} /><FieldLabel htmlFor={`rack-step-${index}`}>{message("Step {index}", { index: plainNumber(index + 1) })}</FieldLabel></Field>
          <NumberField label={message("Step {index} pitch", { index: plainNumber(index + 1) })} value={step.pitchOffset} min={-127} max={127} onChange={(pitchOffset) => update({ ...step, pitchOffset })} />
          <NumberField label={message("Step {index} velocity", { index: plainNumber(index + 1) })} value={step.velocityScale} min={0} max={2} step={0.01} onChange={(velocityScale) => update({ ...step, velocityScale })} />
        </FieldGroup>
      })}</div></FieldGroup>
  }
}
