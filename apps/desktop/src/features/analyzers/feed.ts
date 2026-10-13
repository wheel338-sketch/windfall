import { useEffect, useEffectEvent, useSyncExternalStore } from "react"

import type { EffectId } from "@/bindings"
import { backend } from "@/lib/ipc"
import { onProjectReplaced } from "@/lib/store/replaced"

import type {
  AnalyzerEvent,
  AnalyzerFrame,
  AnalyzerSource,
  AnalyzerStatus,
  AnalyzerWant,
} from "./types"

/*
 * Analyzer frames arrive up to thirty times a second. Like the realtime
 * feed they stay out of React state: a slot keeps its newest frame in a
 * plain object, views add to their own bounded histories as frames arrive
 * and paint inside requestAnimationFrame. React sees only a slot's
 * standing, which changes a few times per selection.
 *
 * A slot is one consumer of analyzer data in this window. Whoever owns a
 * slot selects its source; any number of views may read it. Every frame
 * names the selection it answers, and one that answers an earlier selection
 * is acknowledged, so the next can be sent, and then dropped: a view never
 * draws what it showed a moment ago as what it shows now.
 */

/** The signal scope and the views that share its source. */
export const SCOPE_SLOT = "scope"
/** The effect editor that is open: its EQ spectrum, limiter or tuner. */
export const EFFECT_SLOT = "effect"

export type AnalyzerSelection = {
  source: AnalyzerSource
  effect?: EffectId
  want: AnalyzerWant
}

export type FrameHandlers = {
  /** Called once for every frame of the current selection, in order. */
  onFrame?(frame: AnalyzerFrame): void
  /** What was shown is no longer current: forget it. */
  onClear?(): void
  /** Draw. Called inside requestAnimationFrame after either of the above. */
  paint(frame: AnalyzerFrame | null): void
}

type Slot = {
  name: string
  users: number
  stop: (() => void) | null
  /** Counts selections. Only events that answer the newest one count. */
  request: number
  wanted: AnalyzerSelection | null
  status: AnalyzerStatus | null
  frame: AnalyzerFrame | null
  /** Counts what there is to paint: frames and clears. */
  serial: number
  receivedAt: number
  sequence: bigint
  handlers: Set<FrameHandlers>
  watchers: Set<() => void>
}

const slots = new Map<string, Slot>()

function slotOf(name: string): Slot {
  let slot = slots.get(name)
  if (!slot) {
    slot = {
      name,
      users: 0,
      stop: null,
      request: 0,
      wanted: null,
      status: null,
      frame: null,
      serial: 0,
      receivedAt: 0,
      sequence: 0n,
      handlers: new Set(),
      watchers: new Set(),
    }
    slots.set(name, slot)
  }
  return slot
}

function localStatus(
  slot: Slot,
  phase: AnalyzerStatus["phase"],
  reason: AnalyzerStatus["reason"] = null
): AnalyzerStatus {
  return {
    slot: slot.name,
    request: slot.request,
    phase,
    reason,
    ticket: null,
    source: slot.wanted?.source ?? null,
    effect: slot.wanted?.effect ?? null,
    sourceName: null,
    sampleRate: null,
  }
}

function clear(slot: Slot) {
  slot.frame = null
  slot.serial += 1
  for (const handlers of [...slot.handlers]) handlers.onClear?.()
}

function tell(slot: Slot, status: AnalyzerStatus | null) {
  slot.status = status
  for (const watcher of [...slot.watchers]) watcher()
}

function receive(slot: Slot, event: AnalyzerEvent) {
  if (event.type === "frame") {
    // Always acknowledged, also when it is no longer wanted: the service
    // sends nothing more until it is.
    backend.analyzerAck(slot.name, event.sequence)
    if (event.request !== slot.request || slot.wanted === null) return
    if (!/^[1-9]\d{0,19}$/.test(event.sequence) || BigInt(event.sequence) <= slot.sequence) return
    const standing = slot.status
    if (standing?.phase === "refused" || standing?.phase === "unavailable" || standing?.phase === "waitingForStream") return
    // A live standing authorizes an exact native lease, including recovery.
    // Late channel callbacks must not revive a deleted/replaced owner.
    if (standing?.phase === "live" && standing.ticket &&
      (["service", "subscription", "stream", "selection", "project"] as const).some((key) => standing.ticket?.[key] !== event.ticket[key])) return
    const before = slot.frame
    // Another selection than the frame before, or a break the service
    // reports: nothing shown so far carries on into this frame.
    if (
      event.gap ||
      (before !== null &&
        (before.ticket.selection !== event.ticket.selection ||
          before.ticket.subscription !== event.ticket.subscription))
    ) {
      clear(slot)
    }
    slot.frame = event
    slot.receivedAt = performance.now()
    slot.sequence = BigInt(event.sequence)
    slot.serial += 1
    for (const handlers of [...slot.handlers]) handlers.onFrame?.(event)
    if (slot.status?.phase !== "live") {
      tell(slot, {
        ...(slot.status ?? localStatus(slot, "live")),
        phase: "live",
        reason: null,
        ticket: event.ticket,
        sampleRate: event.clock.sampleRate,
      })
    }
    return
  }
  // A standing for an earlier selection says nothing about this one.
  if (event.request !== slot.request) return
  if (event.phase !== "live") clear(slot)
  tell(slot, event)
}

function available(): boolean {
  // A test that stands in a partial backend has no analyzer at all.
  return typeof backend.analyzerSubscribe === "function"
}

function start(slot: Slot) {
  if (slot.stop || !available()) return
  const subscription = backend.analyzerSubscribe(slot.name, (event) =>
    receive(slot, event)
  )
  slot.stop = subscription.stop
  subscription.status
    .then((status) => {
      if (slot.stop !== subscription.stop) return
      // Nothing is selected yet, or a selection is already on its way and
      // will bring its own standing.
      if (slot.wanted === null || status.phase !== "idle") tell(slot, status)
    })
    .catch(() => {
      if (slot.stop !== subscription.stop) return
      tell(slot, localStatus(slot, "unavailable"))
    })
}

function select(slot: Slot, wanted: AnalyzerSelection | null) {
  slot.wanted = wanted
  slot.request += 1
  slot.sequence = 0n
  const request = slot.request
  clear(slot)
  if (wanted === null) {
    // An empty owner must release its native capture too. Keeping the
    // subscription alive retains the prior source and exhausts capacity.
    slot.stop?.()
    slot.stop = null
    tell(slot, null)
    return
  }
  if (!available()) {
    tell(slot, localStatus(slot, "unavailable", "desktopRequired"))
    return
  }
  start(slot)
  tell(slot, localStatus(slot, "preparing"))
  void (async () => {
    try {
      const guard = await backend.timelineState()
      if (slot.request !== request) return
      const status = await backend.analyzerSelect({
        slot: slot.name,
        request,
        generation: guard.generation,
        source: wanted.source,
        effect: wanted.effect,
        want: wanted.want,
      })
      if (slot.request !== request) return
      // A frame may have arrived first and made the slot live already.
      if (slot.frame === null || status.phase !== "preparing") {
        tell(slot, status)
      }
    } catch {
      if (slot.request !== request) return
      tell(slot, localStatus(slot, "refused", "preparation"))
    }
  })()
}

// The old project's tracks and effects share ids with the new one's. What
// was shown is dropped at once, and each slot asks again for the project
// that is open now. A source that is not in it is refused by the service.
onProjectReplaced(() => {
  for (const slot of slots.values()) {
    if (slot.users > 0 && slot.wanted !== null) select(slot, slot.wanted)
    else clear(slot)
  }
})

function acquire(name: string): () => void {
  const slot = slotOf(name)
  slot.users += 1
  start(slot)
  return () => {
    slot.users -= 1
    if (slot.users > 0) return
    slot.stop?.()
    slot.stop = null
    slot.wanted = null
    slot.request += 1
    clear(slot)
    tell(slot, null)
  }
}

function sameSelection(
  a: AnalyzerSelection | null,
  b: AnalyzerSelection | null
): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}

/**
 * Owns a slot's source while mounted: subscribes it, selects `selection`
 * and selects again whenever that changes. `null` selects nothing. One
 * owner per slot at a time; the one that selected last decides.
 */
export function useAnalyzerSelection(
  name: string,
  selection: AnalyzerSelection | null
) {
  useEffect(() => acquire(name), [name])
  const key = JSON.stringify(selection)
  useEffect(() => {
    const slot = slotOf(name)
    const wanted = JSON.parse(key) as AnalyzerSelection | null
    if (slot.request > 0 && sameSelection(slot.wanted, wanted)) return
    select(slot, wanted)
  }, [name, key])
}

/** A slot's standing, for labels. Changes a few times per selection. */
export function useAnalyzerStatus(name: string): AnalyzerStatus | null {
  return useSyncExternalStore(
    (watcher) => {
      const slot = slotOf(name)
      slot.watchers.add(watcher)
      return () => slot.watchers.delete(watcher)
    },
    () => slotOf(name).status
  )
}

/**
 * Reads a slot's frames while mounted. `onFrame` runs for every frame as it
 * arrives, so a history misses none; `paint` runs at most once per
 * animation frame. Do not set React state in any of them.
 */
export function useAnalyzerFrames(name: string, handlers: FrameHandlers) {
  const onFrame = useEffectEvent((frame: AnalyzerFrame) =>
    handlers.onFrame?.(frame)
  )
  const onClear = useEffectEvent(() => handlers.onClear?.())
  const paint = useEffectEvent((frame: AnalyzerFrame | null) =>
    handlers.paint(frame)
  )
  useEffect(() => {
    const slot = slotOf(name)
    const mine: FrameHandlers = {
      onFrame: (frame) => onFrame(frame),
      onClear: () => onClear(),
      paint: (frame) => paint(frame),
    }
    slot.handlers.add(mine)
    let seen = -1
    let animation = requestAnimationFrame(function draw() {
      animation = requestAnimationFrame(draw)
      if (slot.frame && performance.now() - slot.receivedAt > 500) {
        clear(slot)
        tell(slot, localStatus(slot, "stalled", "audioStalled"))
      }
      if (slot.serial === seen) return
      seen = slot.serial
      paint(slot.frame)
    })
    return () => {
      slot.handlers.delete(mine)
      cancelAnimationFrame(animation)
    }
  }, [name])
}

/** The newest frame of a slot, for code that draws on its own schedule. */
export function analyzerFrame(name: string): AnalyzerFrame | null {
  return slots.get(name)?.frame ?? null
}

/** Hands a slot an event as the backend would. For tests of the views. */
export function deliverAnalyzerEvent(name: string, event: AnalyzerEvent) {
  receive(slotOf(name), event)
}

/** Forgets every slot. For tests. */
export function resetAnalyzerFeed() {
  for (const slot of slots.values()) slot.stop?.()
  slots.clear()
}
