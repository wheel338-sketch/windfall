import { useTranslation } from "@/lib/i18n"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

export type Option<T> = { value: T; label: string }

export function Choice<T extends string | number>({
  id,
  value,
  options,
  disabled,
  onChange,
}: {
  id: string
  value: T
  options: Option<T>[]
  disabled: boolean
  onChange(value: T): void
}) {
  const t = useTranslation()
  const localized = options.map((option) => ({ ...option, label: t(option.label) }))
  return (
    <Select
      items={localized}
      value={value}
      disabled={disabled}
      onValueChange={(next: T | null) => {
        if (next !== null) onChange(next)
      }}
    >
      <SelectTrigger id={id} className="w-full">
        <SelectValue />
      </SelectTrigger>
      <SelectContent alignItemWithTrigger={false}>
        <SelectGroup>
          {localized.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {option.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}
