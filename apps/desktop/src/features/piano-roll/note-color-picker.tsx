import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { isNoteColorGroup, NOTE_COLOR_GROUPS, noteColorGroupLabel } from "@/lib/note-colors"
import { colorToCss } from "@/lib/units"
import { translatedText as t } from "@/lib/i18n"

export function NoteColorPicker({ value, onChange, disabled, mixed = false, compact = false }: {
  value: number | null | "keep"
  onChange(value: number | null | "keep"): void
  disabled?: boolean
  mixed?: boolean
  compact?: boolean
}) {
  const items = [
    ...(mixed ? [{ value: "keep", label: t("Keep each note's color") }] : []),
    { value: "auto", label: noteColorGroupLabel(null) },
    ...NOTE_COLOR_GROUPS.map((_, group) => ({ value: String(group), label: noteColorGroupLabel(group) })),
  ]
  return <Select items={items} value={value === null ? "auto" : String(value)} disabled={disabled} onValueChange={(next) => {
    if (next === "auto") onChange(null)
    else if (next === "keep" && mixed) onChange("keep")
    else if (next !== null && isNoteColorGroup(Number(next))) onChange(Number(next))
  }}>
    <SelectTrigger size={compact ? "sm" : "default"} aria-label={t("Note color and MIDI channel")} className={compact ? "w-44" : undefined}>
      {typeof value === "number" && <span aria-hidden className="size-2 shrink-0 rounded-[2px]" style={{ background: colorToCss(NOTE_COLOR_GROUPS[value]) }} />}
      <SelectValue />
    </SelectTrigger>
    <SelectContent alignItemWithTrigger={false} align="start"><SelectGroup>
      {items.map((item) => <SelectItem key={item.value} value={item.value}>
        {isNoteColorGroup(Number(item.value)) && <span aria-hidden className="size-2 shrink-0 rounded-[2px]" style={{ background: colorToCss(NOTE_COLOR_GROUPS[Number(item.value)]) }} />}
        {item.label}
      </SelectItem>)}
    </SelectGroup></SelectContent>
  </Select>
}
