import { useRef, type CSSProperties } from "react"
import { ArrowDown01Icon, ArrowUp01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import type { Channel, ChannelId } from "@/bindings"
import { noteName } from "@/components/audio"
import { Button } from "@/components/ui/button"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { colorToCss, DEFAULT_KEY, DEFAULT_VELOCITY } from "@/lib/units"

import {
  PAD_COLUMNS,
  PAD_COUNT,
  PAD_ROWS,
  channelBankCount,
  padAtPoint,
  stepPadBase,
} from "./geometry"
import { noteId, useHeldNotes, type HeldNotes } from "./held-notes"
import { useTouchControllersStore } from "./store"

/** What a pad plays and shows. Null is a pad with nothing on it. */
type Pad = {
  channel: ChannelId
  key: number
  /** What is printed on the pad. */
  label: string
  /** The channel color, 0xRRGGBB. */
  color: number
} | null

const KEYBOARD_SOURCE = "keyboard"

/** Pad `n` plays key `base + n` of one channel. */
function notePads(channel: Channel, base: number): Pad[] {
  return Array.from({ length: PAD_COUNT }, (_, index) => ({
    channel: channel.id,
    key: base + index,
    label: noteName(base + index),
    color: channel.color,
  }))
}

/** Pad `n` plays the `n`-th channel of a bank of the rack, in rack order. */
function channelPads(channels: readonly Channel[], bank: number): Pad[] {
  return Array.from({ length: PAD_COUNT }, (_, index) => {
    const channel = channels.at(bank * PAD_COUNT + index)
    return channel
      ? {
          channel: channel.id,
          key: DEFAULT_KEY,
          label: channel.name,
          color: channel.color,
        }
      : null
  })
}

/**
 * Sixteen drum pads. In the Notes layout they play sixteen keys of the
 * target channel; in the Channels layout each plays one channel of the rack.
 */
export function TouchPads({
  channel,
  channels,
}: {
  channel: Channel
  channels: readonly Channel[]
}) {
  const layout = useTouchControllersStore((state) => state.padLayout)
  const setLayout = useTouchControllersStore((state) => state.setPadLayout)
  const base = useTouchControllersStore((state) => state.padBase)
  const shiftBase = useTouchControllersStore((state) => state.shiftPadBase)
  const savedBank = useTouchControllersStore((state) => state.channelBank)
  const setBank = useTouchControllersStore((state) => state.setChannelBank)
  const { notes, held } = useHeldNotes()

  const banks = channelBankCount(channels.length)
  // Channels may have been deleted since the bank was chosen.
  const bank = Math.min(savedBank, banks - 1)
  const byChannel = layout === "channels"
  const pads = byChannel ? channelPads(channels, bank) : notePads(channel, base)
  const first = bank * PAD_COUNT
  const canGoDown = byChannel ? bank > 0 : stepPadBase(base, -1) !== base
  const canGoUp = byChannel ? bank < banks - 1 : stepPadBase(base, 1) !== base

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <ToggleGroup
          aria-label="Pad layout"
          value={[layout]}
          variant="outline"
          size="sm"
          onValueChange={(values) => {
            if (values[0] === "notes" || values[0] === "channels")
              setLayout(values[0])
          }}
        >
          <ToggleGroupItem value="notes">Notes</ToggleGroupItem>
          <ToggleGroupItem value="channels">Channels</ToggleGroupItem>
        </ToggleGroup>
        <div className="flex items-center gap-1">
          <Button
            variant="outline"
            size="icon-lg"
            aria-label="Bank down"
            disabled={!canGoDown}
            onClick={() => (byChannel ? setBank(bank - 1) : shiftBase(-1))}
          >
            <HugeiconsIcon icon={ArrowDown01Icon} strokeWidth={2} />
          </Button>
          <output
            aria-label="Pad bank"
            className="min-w-24 text-center font-readout text-xs"
          >
            {byChannel
              ? `Channels ${first + 1} to ${Math.min(first + PAD_COUNT, channels.length)} of ${channels.length}`
              : `${noteName(base)} to ${noteName(base + PAD_COUNT - 1)}`}
          </output>
          <Button
            variant="outline"
            size="icon-lg"
            aria-label="Bank up"
            disabled={!canGoUp}
            onClick={() => (byChannel ? setBank(bank + 1) : shiftBase(1))}
          >
            <HugeiconsIcon icon={ArrowUp01Icon} strokeWidth={2} />
          </Button>
        </div>
      </div>
      <PadGrid
        label={byChannel ? "Channel pads" : `Note pads, ${channel.name}`}
        pads={pads}
        notes={notes}
        held={held}
      />
    </div>
  )
}

/**
 * The pads as one touch surface. Each pointer holds the pad under it and
 * sliding onto another pad lets go of the first, as on the keyboard. Which
 * pointer is where never renders: only the held notes do.
 */
function PadGrid({
  label,
  pads,
  notes,
  held,
}: {
  label: string
  pads: readonly Pad[]
  notes: HeldNotes
  held: ReadonlySet<string>
}) {
  // The pad each pointer is on, or -1 while it is on none after sliding off.
  const pointers = useRef(new Map<number, number>())
  const source = (pointerId: number) => `pointer:${pointerId}`

  function hit(event: React.PointerEvent<HTMLDivElement>) {
    const target = padAtPoint(
      event.clientX,
      event.clientY,
      event.currentTarget.getBoundingClientRect()
    )
    const pad = target ? pads[target.pad] : null
    return target && pad
      ? { index: target.pad, pad, velocity: target.velocity }
      : null
  }

  function handlePointerDown(event: React.PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return
    const target = hit(event)
    if (!target) return
    try {
      event.currentTarget.setPointerCapture(event.pointerId)
    } catch {
      // Without capture the note still ends when the pointer is lifted.
    }
    pointers.current.set(event.pointerId, target.index)
    notes.press(
      source(event.pointerId),
      target.pad.channel,
      target.pad.key,
      target.velocity
    )
  }

  function handlePointerMove(event: React.PointerEvent<HTMLDivElement>) {
    const current = pointers.current.get(event.pointerId)
    if (current === undefined) return
    const target = hit(event)
    const next = target?.index ?? -1
    if (next === current) return
    pointers.current.set(event.pointerId, next)
    notes.release(source(event.pointerId))
    if (target) {
      notes.press(
        source(event.pointerId),
        target.pad.channel,
        target.pad.key,
        target.velocity
      )
    }
  }

  function handlePointerEnd(event: React.PointerEvent<HTMLDivElement>) {
    pointers.current.delete(event.pointerId)
    notes.release(source(event.pointerId))
  }

  function handleLostCapture(event: React.PointerEvent<HTMLDivElement>) {
    // A touch is captured by the pad it lands on first. That capture moving
    // here is not the end of the touch; only this element losing it is.
    if (event.target === event.currentTarget) handlePointerEnd(event)
  }

  function handleKeyDown(
    event: React.KeyboardEvent<HTMLButtonElement>,
    pad: NonNullable<Pad>
  ) {
    if (event.key !== "Enter" && event.key !== " ") return
    if (event.altKey || event.ctrlKey || event.metaKey) return
    event.preventDefault()
    if (event.repeat) return
    notes.press(KEYBOARD_SOURCE, pad.channel, pad.key, DEFAULT_VELOCITY)
  }

  return (
    <div
      role="group"
      aria-label={label}
      data-slot="touch-pads"
      className="mx-auto grid aspect-square w-full max-w-80 touch-none grid-cols-4 grid-rows-4 select-none"
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerEnd}
      onPointerCancel={handlePointerEnd}
      onLostPointerCapture={handleLostCapture}
      onContextMenu={(event) => event.preventDefault()}
    >
      {pads.map((pad, index) => {
        const name = `Pad ${index + 1}, ${pad ? pad.label : "empty"}`
        return (
          <button
            key={index}
            type="button"
            aria-label={name}
            aria-pressed={
              pad ? held.has(noteId(pad.channel, pad.key)) : undefined
            }
            disabled={!pad}
            data-pad={index}
            className="group/pad touch-none p-0.5 outline-none"
            style={
              {
                // Pad 1 is at the bottom left, as drum pads count.
                gridColumn: (index % PAD_COLUMNS) + 1,
                gridRow: PAD_ROWS - Math.floor(index / PAD_COLUMNS),
                "--pad-color": pad ? colorToCss(pad.color) : undefined,
              } as CSSProperties
            }
            onKeyDown={pad ? (event) => handleKeyDown(event, pad) : undefined}
            onKeyUp={(event) => {
              if (event.key === "Enter" || event.key === " ")
                notes.release(KEYBOARD_SOURCE)
            }}
            onBlur={() => notes.release(KEYBOARD_SOURCE)}
          >
            <span className="pointer-events-none flex size-full flex-col justify-between overflow-hidden rounded-md border border-border bg-[color-mix(in_oklch,var(--pad-color)_24%,var(--muted))] p-1.5 text-left text-foreground group-focus-visible/pad:ring-2 group-focus-visible/pad:ring-ring group-disabled/pad:border-dashed group-disabled/pad:bg-transparent group-disabled/pad:opacity-50 group-aria-pressed/pad:border-(--pad-color) group-aria-pressed/pad:bg-[color-mix(in_oklch,var(--pad-color)_72%,var(--muted))]">
              <span className="font-readout text-[0.625rem] leading-none text-muted-foreground">
                {index + 1}
              </span>
              <span className="truncate text-xs leading-tight font-medium">
                {pad?.label}
              </span>
            </span>
          </button>
        )
      })}
    </div>
  )
}
