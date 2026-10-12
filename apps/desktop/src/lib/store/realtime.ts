import { useEffect, useEffectEvent } from "react"

import type { AutomationId, EffectId, RealtimeFrame } from "@/bindings"
import { backend } from "@/lib/ipc"

import { onProjectReplaced } from "./replaced"

/*
 * Playhead and meters arrive about 60 times a second. Putting them in React
 * state would render the app that often, so they live in a plain object and
 * components draw from it inside requestAnimationFrame.
 */

const latest: RealtimeFrame = {
  playing: false,
  tick: 0,
  meters: [],
  cpu: 0,
  xruns: 0,
  voices: 0,
  gainReductions: [],
  automated: [],
  audioClips: 0,
  droppedClips: 0,
}

type Listener = (frame: Readonly<RealtimeFrame>) => void

const listeners = new Set<Listener>()
let stopFeed: (() => void) | null = null
let animation: number | null = null
/** True when frames have arrived that no listener has drawn yet. */
let undrawn = false
/** The newest gain reduction of each compressor and limiter, in dB. */
const reductions = new Map<EffectId, number>()
/** What each automation is giving its target right now, 0 to 1. */
const automated = new Map<AutomationId, number>()

function receive(frame: RealtimeFrame) {
  latest.playing = frame.playing
  latest.tick = frame.tick
  latest.cpu = frame.cpu
  latest.xruns = frame.xruns
  latest.voices = frame.voices
  latest.rejectedRackEvents = frame.rejectedRackEvents
  latest.rejectedEchoes = frame.rejectedEchoes
  latest.correlation = frame.correlation
  latest.waveforms = frame.waveforms ?? []
  latest.spectrum = frame.spectrum ?? []
  latest.spectrogram = frame.spectrogram ?? []
  // Meters are peaks since the previous frame. When the screen draws slower
  // than frames arrive, keep the highest so a short hit is not skipped.
  if (undrawn && latest.meters.length === frame.meters.length) {
    for (let index = 0; index < frame.meters.length; index += 1) {
      latest.meters[index] = Math.max(latest.meters[index], frame.meters[index])
    }
  } else {
    latest.meters = frame.meters.slice()
  }
  // Gain reductions are the deepest since the previous frame, so the same
  // goes for them: between two draws the deepest one is kept.
  if (!undrawn) reductions.clear()
  for (const { effect, db } of frame.gainReductions) {
    reductions.set(effect, Math.max(reductions.get(effect) ?? 0, db))
  }
  latest.gainReductions = frame.gainReductions
  // Where a control is right now, so only the newest value counts.
  latest.automated = frame.automated
  automated.clear()
  for (const { automation, value } of frame.automated) {
    automated.set(automation, value)
  }
  undrawn = true
}

// Levels and reductions still waiting to be drawn are the old project's,
// and its tracks and effects share ids with the new one's.
onProjectReplaced(() => {
  latest.playing = false
  latest.tick = 0
  latest.meters = []
  latest.rejectedRackEvents = undefined
  latest.rejectedEchoes = undefined
  latest.correlation = undefined
  latest.waveforms = []
  latest.spectrum = []
  latest.spectrogram = []
  latest.gainReductions = []
  latest.automated = []
  reductions.clear()
  automated.clear()
  undrawn = false
})

function draw() {
  animation = requestAnimationFrame(draw)
  undrawn = false
  for (const listener of [...listeners]) listener(latest)
}

function start() {
  stopFeed ??= backend.subscribeRealtime(receive)
  animation ??= requestAnimationFrame(draw)
}

function stop() {
  stopFeed?.()
  stopFeed = null
  if (animation !== null) cancelAnimationFrame(animation)
  animation = null
}

/**
 * Calls `listener` once per animation frame with the newest values. The
 * frame object is reused, so read from it and do not keep it.
 */
export function subscribeRealtime(listener: Listener): () => void {
  listeners.add(listener)
  start()
  return () => {
    listeners.delete(listener)
    if (listeners.size === 0) stop()
  }
}

/** The newest frame, for code that draws on its own schedule. */
export function realtimeFrame(): Readonly<RealtimeFrame> {
  return latest
}

/**
 * Runs `draw` once per animation frame with the newest realtime values. Draw
 * to a canvas or set a style in it; do not set React state.
 */
export function useRealtime(draw: Listener) {
  const onFrame = useEffectEvent(draw)
  useEffect(() => subscribeRealtime((frame) => onFrame(frame)), [])
}

/**
 * Peak levels of one mixer track as linear gain, left and right.
 * `trackIndex` is the track's position in the mixer; the master is 0.
 */
export function useMeter(
  trackIndex: number,
  draw: (left: number, right: number) => void
) {
  useRealtime((frame) => {
    const left = frame.meters[trackIndex * 2] ?? 0
    const right = frame.meters[trackIndex * 2 + 1] ?? 0
    draw(left, right)
  })
}

/**
 * A feed of one mixer track's peak levels, in the shape the level meter in
 * `components/audio` takes as its `subscribe` prop:
 *
 *     <LevelMeter subscribe={meterFeed(trackIndex)} />
 */
export function meterFeed(trackIndex: number) {
  return (listener: (left: number, right: number) => void) =>
    subscribeRealtime((frame) =>
      listener(
        frame.meters[trackIndex * 2] ?? 0,
        frame.meters[trackIndex * 2 + 1] ?? 0
      )
    )
}

/**
 * A feed of how far one compressor or limiter is turning its signal down,
 * in dB, 0 or more: the deepest reduction since the last animation frame.
 * An effect the engine does not report reads 0.
 *
 *     gainReductionFeed(slot.id)((db) => bar.style.height = `${db * 4}px`)
 */
export function gainReductionFeed(effect: EffectId) {
  return (listener: (db: number) => void) =>
    subscribeRealtime(() => listener(reductions.get(effect) ?? 0))
}

/**
 * Runs `draw` once per animation frame with the gain reduction of one
 * compressor or limiter in dB. Draw to a canvas or set a style in it; do
 * not set React state.
 */
export function useGainReduction(effect: EffectId, draw: (db: number) => void) {
  useRealtime(() => draw(reductions.get(effect) ?? 0))
}

/**
 * The value, 0 to 1, an automation is giving its target in the newest
 * frame, or undefined when it does not have its target in hand: while the
 * song is stopped, in pattern mode, and before its first clip.
 */
export function automatedValue(automation: AutomationId): number | undefined {
  return automated.get(automation)
}

/**
 * The playhead in ticks. In pattern mode it is the position inside the
 * pattern, in song mode the position on the playlist.
 */
export function usePlayhead(draw: (tick: number, playing: boolean) => void) {
  useRealtime((frame) => draw(frame.tick, frame.playing))
}
