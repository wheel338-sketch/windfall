import type { ChannelId } from "@/bindings"
import { onProjectReplaced } from "@/lib/store/replaced"

/*
 * The keys the user is holding right now, per channel, as the audition path
 * reports them: the typing keyboard, step entry, the key gutter and the note
 * being drawn or dragged in the grid. A key counts once however many of
 * those hold it, and the first release lets it go, as the note log does.
 * Outside the document, persistence and undo history.
 */
const NONE: readonly number[] = Object.freeze([])
const held = new Map<ChannelId, Set<number>>()
// Hardware events describe sounding keys (including sustained keys). Keep
// their ownership separate from typing/gutter auditions on the same pitch.
const hardware = new Map<ChannelId, Set<number>>()
// Sorted keys per channel, kept until that channel changes, so a snapshot
// read twice is the same array.
const sorted = new Map<ChannelId, readonly number[]>()
const listeners = new Set<() => void>()

function changed(channel?: ChannelId) {
  if (channel === undefined) sorted.clear()
  else sorted.delete(channel)
  for (const listener of [...listeners]) listener()
}

export function holdKey(channel: ChannelId, key: number): void {
  if (!Number.isInteger(key) || key < 0 || key > 127) return
  let keys = held.get(channel)
  if (!keys) held.set(channel, (keys = new Set()))
  if (keys.has(key)) return
  keys.add(key)
  changed(channel)
}

/** Native accepted down/up feed; repeated down is idempotent. */
export function hardwareHeldKey(channel: ChannelId, key: number, velocity: number): void {
  if (!Number.isInteger(key) || key < 0 || key > 127 || !Number.isFinite(velocity)) return
  let keys = hardware.get(channel)
  if (velocity > 0) {
    if (!keys) hardware.set(channel, (keys = new Set()))
    if (keys.has(key)) return
    keys.add(key)
  } else {
    if (!keys?.delete(key)) return
    if (!keys.size) hardware.delete(channel)
  }
  changed(channel)
}

export function releaseKey(channel: ChannelId, key: number): void {
  const keys = held.get(channel)
  if (!keys?.delete(key)) return
  if (keys.size === 0) held.delete(channel)
  changed(channel)
}

/** Hardware panic/input teardown must preserve independent UI auditions. */
export function releaseHardwareKeys(): void {
  if (!hardware.size) return
  hardware.clear()
  changed()
}

/**
 * Lets go of every key of one channel, or of all channels. For anything
 * that silences auditioned notes without a release for each: a panic, a
 * project being replaced, the readout going away.
 */
export function releaseHeldKeys(channel?: ChannelId): void {
  if (channel === undefined) {
    if (held.size === 0 && hardware.size === 0) return
    held.clear()
    hardware.clear()
    changed()
    return
  }
  const auditionChanged = held.delete(channel)
  const hardwareChanged = hardware.delete(channel)
  if (auditionChanged || hardwareChanged) changed(channel)
}

/** The held keys of a channel, ascending. The same array until they change. */
export function heldKeys(channel: ChannelId | null | undefined): readonly number[] {
  if (channel === null || channel === undefined) return NONE
  const keys = held.get(channel)
  const native = hardware.get(channel)
  if (!keys && !native) return NONE
  let list = sorted.get(channel)
  if (!list) {
    list = [...new Set([...(keys ?? []), ...(native ?? [])])].sort((a, b) => a - b)
    sorted.set(channel, list)
  }
  return list
}

export function subscribeHeldKeys(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

// Channel ids start over in the next project.
onProjectReplaced(() => releaseHeldKeys())
