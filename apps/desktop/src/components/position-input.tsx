import { useState } from "react"

import { Input } from "@/components/ui/input"
import { formatPositionText, parsePositionText } from "@/lib/position-text"
import { useProjectStore } from "@/lib/store/project"

type PositionInputProps = Omit<
  React.ComponentProps<"input">,
  "type" | "value" | "defaultValue" | "onChange"
> & {
  /** A song tick, or `undefined` for none. */
  value: number | undefined
  onValueChange(tick: number | undefined): void
  /** Lets an empty field mean "no position". */
  optional?: boolean
}

/**
 * A song tick typed and shown as `bar:beat:tick` in the song's meter, so
 * nobody has to work out ticks. The stored value stays a tick. Text that is
 * not a position yet is kept while typing and marked invalid; leaving the
 * field shows the last position again.
 */
export function PositionInput({
  value,
  onValueChange,
  optional = false,
  onBlur,
  ...props
}: PositionInputProps) {
  const signature = useProjectStore(
    (state) => state.project.settings.timeSignature
  )
  const meters = useProjectStore(
    (state) => state.project.playlist.timeline?.meters
  )
  const [draft, setDraft] = useState<string | null>(null)
  const read = (text: string) =>
    text.trim() === ""
      ? optional
        ? undefined
        : null
      : parsePositionText(text, signature, meters ?? [])
  const text =
    draft ??
    (value === undefined ? "" : formatPositionText(value, signature, meters ?? []))

  return (
    <Input
      {...props}
      type="text"
      autoComplete="off"
      spellCheck={false}
      value={text}
      aria-invalid={read(text) === null || props["aria-invalid"]}
      onChange={(event) => {
        setDraft(event.target.value)
        const tick = read(event.target.value)
        if (tick !== null) onValueChange(tick)
      }}
      onBlur={(event) => {
        setDraft(null)
        onBlur?.(event)
      }}
    />
  )
}
