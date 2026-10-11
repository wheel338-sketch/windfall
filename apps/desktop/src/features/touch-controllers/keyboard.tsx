import { useMemo } from "react"
import {
  ArrowLeft01Icon,
  ArrowRight01Icon,
} from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import type { Channel } from "@/bindings"
import { noteName, PianoKeyboard } from "@/components/audio"
import { Button } from "@/components/ui/button"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { colorToCss } from "@/lib/units"

import {
  OCTAVE_RANGES,
  isOctaveRange,
  keyboardSpan,
  maxLowKey,
} from "./geometry"
import { noteId, useHeldNotes } from "./held-notes"
import { useTouchControllersStore } from "./store"

/**
 * A keyboard for fingers: every touch holds its own key, and sliding plays
 * the keys on the way. `PianoKeyboard` tracks the pointers; this adds the
 * octave controls and sends the notes to the channel.
 */
export function TouchKeyboard({ channel }: { channel: Channel }) {
  const lowKey = useTouchControllersStore((state) => state.lowKey)
  const octaves = useTouchControllersStore((state) => state.octaves)
  const shiftOctave = useTouchControllersStore((state) => state.shiftOctave)
  const setOctaves = useTouchControllersStore((state) => state.setOctaves)
  const { notes, held } = useHeldNotes()
  const { low, high } = keyboardSpan(lowKey, octaves)
  const active = useMemo(() => {
    const keys: number[] = []
    for (let key = low; key <= high; key += 1) {
      if (held.has(noteId(channel.id, key))) keys.push(key)
    }
    return keys
  }, [held, channel.id, low, high])

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <div className="flex items-center gap-1">
          <Button
            variant="outline"
            size="icon-lg"
            aria-label="Octave down"
            disabled={low <= 0}
            onClick={() => shiftOctave(-1)}
          >
            <HugeiconsIcon icon={ArrowLeft01Icon} strokeWidth={2} />
          </Button>
          <output
            aria-label="Key range"
            className="min-w-20 text-center font-readout text-xs"
          >
            {noteName(low)} to {noteName(high)}
          </output>
          <Button
            variant="outline"
            size="icon-lg"
            aria-label="Octave up"
            disabled={low >= maxLowKey(octaves)}
            onClick={() => shiftOctave(1)}
          >
            <HugeiconsIcon icon={ArrowRight01Icon} strokeWidth={2} />
          </Button>
        </div>
        <ToggleGroup
          aria-label="Octaves shown"
          value={[String(octaves)]}
          variant="outline"
          size="sm"
          onValueChange={(values) => {
            const next = Number(values[0])
            if (isOctaveRange(next)) setOctaves(next)
          }}
        >
          {OCTAVE_RANGES.map((range) => (
            <ToggleGroupItem
              key={range}
              value={String(range)}
              aria-label={`${range} octaves`}
            >
              {range}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>
      <PianoKeyboard
        aria-label={`Touch keyboard, ${channel.name}`}
        lowKey={low}
        highKey={high}
        activeKeys={active}
        color={colorToCss(channel.color)}
        onNoteOn={(key, velocity) =>
          notes.press(`key:${key}`, channel.id, key, velocity)
        }
        onNoteOff={(key) => notes.release(`key:${key}`)}
        className="h-36"
      />
    </div>
  )
}
