import { colorToCss } from "@/lib/units"
import { cn } from "@/lib/utils"
import { translatedText as t } from "@/lib/i18n"

/** Colors a channel can be given, in hue order with a few neutrals last. */
export const CHANNEL_COLORS: { color: number; name: string }[] = [
  { color: 0xe5484d, name: "Red" },
  { color: 0xf76b15, name: "Orange" },
  { color: 0xffb224, name: "Amber" },
  { color: 0x99d52a, name: "Lime" },
  { color: 0x46a758, name: "Green" },
  { color: 0x12a594, name: "Teal" },
  { color: 0x05a2c2, name: "Cyan" },
  { color: 0x3e63dd, name: "Blue" },
  { color: 0x6e56cf, name: "Violet" },
  { color: 0x8e4ec6, name: "Purple" },
  { color: 0xd6409f, name: "Pink" },
  { color: 0xe93d82, name: "Crimson" },
  { color: 0xad7f58, name: "Brown" },
  { color: 0x5f7a8c, name: "Slate" },
  { color: 0x8b8d98, name: "Gray" },
  { color: 0xd9d9e0, name: "Silver" },
]

type ColorSwatchesProps = {
  value: number
  onPick(color: number): void
  className?: string
}

/** A small grid of colors to choose one from. */
export function ColorSwatches({
  value,
  onPick,
  className,
}: ColorSwatchesProps) {
  return (
    <div
      role="group"
      aria-label={t("Channel color")}
      className={cn("grid grid-cols-8 gap-1", className)}
    >
      {CHANNEL_COLORS.map(({ color, name }) => (
        <button
          key={color}
          type="button"
          aria-label={name}
          aria-pressed={color === value}
          title={name}
          onClick={() => onPick(color)}
          className="size-5 rounded-[3px] ring-offset-2 ring-offset-popover outline-none hover:brightness-110 focus-visible:ring-2 focus-visible:ring-ring aria-pressed:ring-2 aria-pressed:ring-foreground"
          style={{ backgroundColor: colorToCss(color) }}
        />
      ))}
    </div>
  )
}
