import type { ReactNode } from "react"
import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"

export function MediaNumber({ label, value, min = 0, max, step = 1, onCommit }: { label: string; value: number; min?: number; max?: number; step?: number; onCommit(value: number): void }) {
  return <Field><FieldLabel>{label}<Input key={value} aria-label={label} type="number" min={min} max={max} step={step} defaultValue={value} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur() }} onBlur={(event) => {
    const next = Number(event.currentTarget.value)
    if (event.currentTarget.value !== "" && Number.isFinite(next) && next >= min && (max === undefined || next <= max) && next !== value) onCommit(next)
    else event.currentTarget.value = String(value)
  }} /></FieldLabel></Field>
}

export function MediaChoice<Value extends string>({ label, value, options, onChange, disabled = false }: { label: string; value: Value; options: readonly { value: Value; label: ReactNode }[]; onChange(value: Value): void; disabled?: boolean }) {
  return <Field><FieldLabel>{label}</FieldLabel><Select items={options} disabled={disabled} value={value} onValueChange={(next) => { if (next !== null) onChange(next as Value) }}><SelectTrigger aria-label={label}><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{options.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}</SelectGroup></SelectContent></Select></Field>
}
