import { Fragment, memo, useEffect, useRef } from "react"

import type { EffectId, TrackId } from "@/bindings"
import { ToggleLed } from "@/components/audio"
import { ContextActions } from "@/components/context-actions"
import { GainReductionBar } from "@/features/effects"
import { pluginDropHandlers } from "@/features/plugins/insert"
import { usePluginBinding } from "@/features/plugins/store"
import { useShortcutScope } from "@/lib/actions"
import { useHint } from "@/lib/store"
import { cn } from "@/lib/utils"

import { endEffectDrag, startEffectDrag } from "./effect-drag"
import { AddEffectMenu, effectMenu } from "./effect-menu"
import {
  effectName,
  hasGainReduction,
  MAX_EFFECT_SLOTS,
  openChain,
  openEffect,
  selectEffect,
  setEffectEnabled,
} from "./effect-ops"
import { useEffectsUi } from "./effects-ui"
import { EFFECT_ROW_HEIGHT } from "./layout"
import { useEffectCount, useEffectIds, useSlotView } from "./strip-track"

/** The line that shows where a dragged effect would land. */
export function DropLine() {
  return (
    <li
      aria-hidden
      data-slot="effect-drop"
      className="relative col-span-full h-0 list-none"
    >
      <span className="pointer-events-none absolute inset-x-0 -top-px z-10 h-0.5 rounded-full bg-brand" />
    </li>
  )
}

type SlotProps = { track: TrackId; effect: EffectId }

/**
 * One effect on a strip: a lamp that switches it on and off, and its name,
 * which opens its editor. It reads only what it shows, so turning a knob
 * in the editor does not render it.
 */
const EffectSlotRow = memo(function EffectSlotRow({
  track,
  effect,
}: SlotProps) {
  const view = useSlotView(track, effect)
  const plugin = usePluginBinding({ type: "effect", effect })
  const selected = useEffectsUi((state) => state.selectedEffect === effect)
  const focusing = useEffectsUi((state) => state.focusing === effect)
  const open = useRef<HTMLButtonElement>(null)
  const hint = useHint(
    "Click to edit, drag to reorder or to another track (Ctrl copies). Alt+↑ or ↓ moves it, Delete removes it. Right-click for more"
  )
  // Delete means "this effect" while a slot has the focus and "this track"
  // anywhere else in the strip, so a slot is a scope of its own inside the
  // mixer's, with the effect actions' keys in it.
  const scope = useShortcutScope("effect")

  useEffect(() => {
    if (!focusing) return
    open.current?.focus({ preventScroll: true })
    useEffectsUi.setState({ focusing: null })
  }, [focusing])

  if (!view) return null
  const name = plugin?.name ?? effectName(view.kind)

  return (
    <ContextActions items={effectMenu(view.kind, !!plugin)}>
      <li
        data-slot="effect-slot"
        {...pluginDropHandlers({ track, effect })}
        data-effect-row={effect}
        data-kind={view.kind}
        data-bypassed={view.enabled ? undefined : ""}
        data-selected={selected || undefined}
        draggable
        className="group/slot relative flex shrink-0 items-center gap-0.5 rounded-[3px] pl-0.5 hover:bg-muted data-selected:bg-[color-mix(in_oklch,var(--wf-brand)_16%,transparent)]"
        style={{ height: EFFECT_ROW_HEIGHT }}
        // The menu's entries act on the selected effect.
        onContextMenu={() => selectEffect(effect)}
        onDragStart={(event) => {
          selectEffect(effect)
          startEffectDrag(event, effect)
        }}
        onDragEnd={endEffectDrag}
        {...hint}
        {...scope}
      >
        <ToggleLed
          variant="dot"
          size="sm"
          tabIndex={-1}
          pressed={view.enabled}
          color="var(--wf-ok)"
          aria-label={`${name} on`}
          className="relative after:absolute after:-inset-1"
          onPressedChange={(on) => void setEffectEnabled(effect, on)}
        />
        <button
          ref={open}
          type="button"
          data-slot="effect-open"
          title={name}
          className="h-full min-w-0 flex-1 truncate rounded-[2px] text-left text-[10px] leading-none tracking-[-0.03em] outline-none group-data-bypassed/slot:text-muted-foreground group-data-bypassed/slot:line-through group-data-bypassed/slot:decoration-muted-foreground/50 focus-visible:ring-2 focus-visible:ring-ring"
          onClick={() => openEffect(effect)}
          onFocus={() => selectEffect(effect)}
        >
          {name}
        </button>
        {hasGainReduction(view.kind) && (
          <GainReductionBar
            effect={effect}
            className="absolute right-0.5 bottom-0 left-3"
          />
        )}
      </li>
    </ContextActions>
  )
})

type EffectRackProps = {
  track: TrackId
  /** Rows the rack is tall. A longer chain scrolls inside it. */
  rows: number
  /** The gap a dragged effect would land in, when one is over the strip. */
  dropGap: number | null
  className?: string
}

/**
 * The effects of a track, in the order the signal runs through them, with
 * a row to add one. Ten fit; a full chain says so where the add row was.
 */
export const EffectRack = memo(function EffectRack({
  track,
  rows,
  dropGap,
  className,
}: EffectRackProps) {
  const ids = useEffectIds(track)
  const full = ids.length >= MAX_EFFECT_SLOTS

  return (
    <ul
      aria-label="Effects"
      data-slot="effect-rack"
      className={cn(
        "ml-0.5 flex shrink-0 flex-col overflow-x-hidden overflow-y-auto overscroll-contain",
        className
      )}
      style={{ height: rows * EFFECT_ROW_HEIGHT }}
    >
      {ids.map((id, index) => (
        <Fragment key={id}>
          {dropGap === index && <DropLine />}
          <EffectSlotRow track={track} effect={id} />
        </Fragment>
      ))}
      {dropGap === ids.length && <DropLine />}
      <li {...pluginDropHandlers({ track })} className="flex shrink-0" style={{ height: EFFECT_ROW_HEIGHT }}>
        {full ? (
          <span
            data-slot="effect-rack-full"
            title={`A track holds ${MAX_EFFECT_SLOTS} effects at most`}
            className="flex h-full items-center pl-1 font-readout text-[9px] text-muted-foreground"
          >
            {ids.length}/{MAX_EFFECT_SLOTS}
          </span>
        ) : (
          <AddEffectMenu track={track} />
        )}
      </li>
    </ul>
  )
})

type EffectBadgeProps = {
  track: TrackId
  /** Show the badge on a track with no effects too. */
  showEmpty: boolean
  className?: string
}

/**
 * What is left of the rack when the strip is too low for it: "FX" and the
 * number of effects. A click opens the chain in the inspector.
 */
export function EffectBadge({ track, showEmpty, className }: EffectBadgeProps) {
  const count = useEffectCount(track)
  const hint = useHint(
    count === 0
      ? "No effects on this track. Click to add one"
      : "Click to see this track's effects"
  )
  if (count === 0 && !showEmpty) return null
  return (
    <button
      type="button"
      data-slot="effect-badge"
      data-empty={count === 0 ? "" : undefined}
      aria-label={count === 1 ? "FX: 1 effect" : `FX: ${count} effects`}
      className={cn(
        "flex h-4 shrink-0 items-center gap-0.5 rounded-[3px] bg-muted px-1 font-readout text-[9px] leading-none text-foreground outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring data-empty:bg-transparent data-empty:text-muted-foreground data-empty:hover:bg-muted",
        className
      )}
      onClick={() => openChain(track)}
      {...hint}
    >
      {count === 0 ? "+ FX" : "FX"}
      {count > 0 && (
        <span data-slot="effect-count" className="text-(--wf-brand)">
          {count}
        </span>
      )}
    </button>
  )
}
