import type {
  AutomatedValue,
  Pattern,
  Project,
  RealtimeFrame,
  TransportPatch,
  TransportState,
} from "@/bindings"
import { automatedAt, compileLanes, type Lane } from "@/lib/automation/lanes"
import { curveValue } from "@/lib/automation/curve"
import { songTempoMap } from "@/lib/automation/tempo-map"
import { soloSet } from "@/lib/mixer-graph"
import { playedNoteTiming } from "@/lib/note-timing"
import { MASTER_TRACK, PPQ, TICKS_PER_STEP } from "@/lib/units"

import { simulatedGainReductions } from "./effects"

type Onset = { channel: number; velocity: number }

/** How fast a simulated meter falls back, as a time constant in seconds. */
const METER_FALL_SECS = 0.14

/** The highest level a simulated meter shows. */
const METER_CEILING = 1.4

/** The level a simulated audio clip plays at, at unity gain. */
const AUDIO_CLIP_LEVEL = 0.55

function patternOnsets(project: Project, pattern: Pattern, from: number, to: number): Onset[] {
  const onsets: Onset[] = []
  for (const lane of pattern.lanes) {
    const channel = project.channels.find((channel) => channel.id === lane.channel)
    if (!channel) continue
    for (const note of lane.notes) {
      if (note.expression?.articulation === "slide") continue
      const timing = playedNoteTiming(channel, note, pattern.lengthSteps * TICKS_PER_STEP, project.settings.swing)
      if (timing && timing.start >= from && timing.start < to) {
        onsets.push({ channel: lane.channel, velocity: note.velocity })
      }
    }
  }
  return onsets
}

/**
 * Where the song ends: the end of its last clip, and 0 with no clips. A
 * muted clip still takes up room, as it does in the engine.
 */
function songLength(project: Project): number {
  return project.playlist.clips.reduce(
    (latest, clip) => Math.max(latest, clip.start + clip.length),
    0
  )
}

/**
 * Where the song is `seconds` after it was at `from`. The song's clock
 * follows the tempo automation, as the engine's does.
 */
function songPosition(project: Project, from: number, seconds: number): number {
  const map = songTempoMap(project)
  if (map.steady) {
    return from + (seconds * project.settings.tempoBpm * PPQ) / 60
  }
  return map.tickAt(map.secondsAt(from) + seconds)
}

function songOnsets(project: Project, from: number, to: number): Onset[] {
  const onsets: Onset[] = []
  for (const clip of project.playlist.clips) {
    const track = project.playlist.tracks.find((item) => item.id === clip.track)
    if (clip.muted || track?.muted) continue
    if (clip.start >= to || clip.start + clip.length <= from) continue
    const content = clip.content
    if (content.type !== "pattern") continue
    const pattern = project.patterns.find((item) => item.id === content.pattern)
    if (!pattern) continue
    const length = pattern.lengthSteps * TICKS_PER_STEP
    for (const lane of pattern.lanes) {
      const channel = project.channels.find((channel) => channel.id === lane.channel)
      if (!channel) continue
      for (const note of lane.notes) {
        if (note.expression?.articulation === "slide") continue
        const timing = playedNoteTiming(channel, note, length, project.settings.swing)
        if (!timing) continue
        // The clip loops its pattern, so a note sounds once per pass.
        let at = clip.start + timing.start - (clip.offset % length)
        if (at < clip.start) at += length
        for (; at < clip.start + clip.length; at += length) {
          if (at >= from && at < to) {
            onsets.push({ channel: lane.channel, velocity: note.velocity })
          }
        }
      }
    }
  }
  return onsets
}

/**
 * The audio clips that sound at a tick of the song, as the mixer track each
 * plays into and its level. A clip is taken to sound for its whole length:
 * the mock has no audio to run out.
 */
function audioClipLevels(project: Project, tick: number): [number, number][] {
  const muted = new Set(
    project.playlist.tracks
      .filter((track) => track.muted)
      .map((track) => track.id)
  )
  const levels: [number, number][] = []
  for (const clip of project.playlist.clips) {
    if (clip.content.type !== "audio") continue
    if (clip.muted || muted.has(clip.track)) continue
    if (tick < clip.start || tick >= clip.start + clip.length) continue
    // A little movement, so a held clip does not read as a stuck meter.
    const wobble = 0.85 + 0.15 * Math.abs(Math.sin(tick / 173))
    levels.push([
      clip.content.mixerTrack,
      AUDIO_CLIP_LEVEL * clip.content.gain * wobble,
    ])
  }
  return levels
}

/**
 * A stand-in for the engine's transport and meters: a playhead that follows
 * the tempo, and meters that jump when a note starts and then fall.
 */
export class TransportSim {
  region: import("@/bindings").TickRange | null = null
  navigationOverflows = 0
  private ignoredPause: number | null = null
  setRegion(region: import("@/bindings").TickRange | null, project: Project) {
    this.region = region
    this.ignoredPause = null
    if (
      region &&
      this.state.mode === "song" &&
      (this.tick < region.start || this.tick >= region.end)
    )
      this.seek(region.start, project)
  }
  state: TransportState
  /** The playhead. */
  tick = 0
  /**
   * Where playback last started or was moved to by a seek. Stopping
   * returns the playhead here, and the next play starts here.
   */
  private position = 0
  private levels = new Map<number, number>()
  /** Notes played by hand since the last frame. */
  private struck: Onset[] = []
  /** Keys held by hand and their velocities, by channel. */
  private held = new Map<number, Map<number, number>>()
  /** The automation lanes of the project they were last made for. */
  private lanes: {
    playlist: Project["playlist"]
    automations: Project["automations"]
    patterns: Project["patterns"]
    lanes: Lane[]
  } | null = null

  constructor(pattern: number, loopSong = false) {
    this.state = { playing: false, mode: "pattern", pattern, loopSong }
  }

  play(project: Project) {
    if (this.state.playing) return
    this.state = { ...this.state, playing: true }
    this.tick = this.position
    if (this.inSong() && !this.region && this.tick === 0) {
      const start = project.playlist.timeline?.markers
        .filter((marker) => marker.kind.type === "start")
        .at(-1)
      if (start) this.tick = start.tick
    }
    this.keepInRange(project)
  }

  stop() {
    this.state = { ...this.state, playing: false }
    this.tick = this.position
  }

  seek(tick: number, project: Project) {
    this.ignoredPause = null
    if (this.state.mode === "song" && this.region)
      tick = Math.max(this.region.start, Math.min(tick, this.region.end))
    this.position = Number.isFinite(tick) ? Math.max(0, tick) : 0
    this.tick = this.position
    this.keepInRange(project)
  }

  set(patch: TransportPatch, project: Project) {
    const modeChanged =
      patch.mode !== undefined && patch.mode !== this.state.mode
    this.state = {
      ...this.state,
      mode: patch.mode ?? this.state.mode,
      pattern: patch.pattern ?? this.state.pattern,
      loopSong: patch.loopSong ?? this.state.loopSong,
      metronome: patch.metronome ?? this.state.metronome,
      countInBars: patch.countInBars ?? this.state.countInBars,
    }
    // A place in a pattern means nothing on the playlist, and the other
    // way round.
    if (modeChanged) {
      this.position = 0
      this.tick = 0
    }
    this.keepInRange(project)
  }

  /** A key played by hand: from the rack, the keyboard or the piano roll. */
  noteOn(channel: number, key: number, velocity: number) {
    this.struck.push({ channel, velocity })
    const keys = this.held.get(channel) ?? new Map<number, number>()
    keys.set(key, velocity)
    this.held.set(channel, keys)
  }

  noteOff(channel: number, key: number) {
    const keys = this.held.get(channel)
    if (!keys?.delete(key)) return
    if (keys.size === 0) this.held.delete(channel)
  }

  /** Moves time forward and reports what the engine would. */
  advance(project: Project, seconds: number): RealtimeFrame {
    const onsets = this.state.playing ? this.step(project, seconds) : []
    onsets.push(...this.struck)
    this.struck = []
    // An instrument sounds for as long as a key is down. A sampler hit has
    // already been counted when it was struck.
    for (const [channel, keys] of this.held) {
      const source = project.channels.find((item) => item.id === channel)
      if (source?.source.type !== "instrument") continue
      onsets.push({ channel, velocity: Math.max(...keys.values()) })
    }
    const fall = Math.exp(-seconds / METER_FALL_SECS)
    for (const [id, level] of this.levels) {
      this.levels.set(id, level < 0.001 ? 0 : level * fall)
    }

    const tracks = new Set(project.mixer.tracks.map((track) => track.id))
    const anySolo = project.channels.some((channel) => channel.solo)
    for (const onset of onsets) {
      const channel = project.channels.find((item) => item.id === onset.channel)
      if (!channel || channel.muted || (anySolo && !channel.solo)) continue
      if (
        project.plugins?.some(
          (plugin) =>
            plugin.target.type === "instrument" &&
            plugin.target.channel === channel.id
        )
      )
        continue
      // A sampler has its sample gain, an instrument its output level.
      const gain =
        channel.source.type === "sampler"
          ? channel.source.gain
          : "gain" in channel.source.params ? channel.source.params.gain : 1
      const hit = onset.velocity * channel.volume * gain
      // A channel whose track is gone plays into the master.
      const track = tracks.has(channel.mixerTrack)
        ? channel.mixerTrack
        : MASTER_TRACK
      this.levels.set(track, Math.max(this.levels.get(track) ?? 0, hit))
    }
    if (this.inSong()) {
      for (const [id, level] of audioClipLevels(project, this.tick)) {
        const track = tracks.has(id) ? id : MASTER_TRACK
        this.levels.set(track, Math.max(this.levels.get(track) ?? 0, level))
      }
    }

    const meters = this.meters(project)
    const voices = [...this.levels.values()].filter((level) => level > 0.05)
    return {
      playing: this.state.playing,
      tick: this.tick,
      meters,
      cpu: 0.03 + voices.length * 0.012 + Math.random() * 0.006,
      xruns: 0,
      voices: voices.length,
      gainReductions: simulatedGainReductions(
        project,
        (track) => meters[track * 2] ?? 0
      ),
      automated: this.automated(project),
      // The mock plays every clip; it has no limit to run into.
      audioClips: 0,
      droppedClips: 0,
    }
  }

  /** True while the song, not a pattern, is playing. */
  private inSong(): boolean {
    return this.state.playing && this.state.mode === "song"
  }

  /**
   * What automation is doing at the playhead, read from the curves the way
   * the engine reads them. Nothing while stopped and in pattern mode.
   */
  private automated(project: Project): AutomatedValue[] {
    if (!this.state.playing) return []
    if (this.state.mode === "pattern") {
      const pattern = project.patterns.find((pattern) => pattern.id === this.state.pattern) ?? project.patterns[0]
      return (pattern?.events ?? []).slice(0, 128).map((lane) => ({ automation: lane.id, value: curveValue(lane.points, this.tick) }))
    }
    const kept = this.lanes
    const lanes =
      kept &&
      kept.playlist === project.playlist &&
      kept.patterns === project.patterns &&
      kept.automations === project.automations
        ? kept.lanes
        : compileLanes(project)
    this.lanes = {
      playlist: project.playlist,
      automations: project.automations,
      patterns: project.patterns,
      lanes,
    }
    return automatedAt(
      lanes,
      [...project.automations.map((automation) => automation.id), ...project.patterns.flatMap((pattern) => (pattern.events ?? []).map((lane) => lane.id))],
      this.tick
    )
  }

  /** Length in ticks of what is playing: the pattern, or the whole song. */
  private length(project: Project): number {
    if (this.state.mode === "song") return songLength(project)
    const pattern =
      project.patterns.find((item) => item.id === this.state.pattern) ??
      project.patterns[0]
    return pattern.lengthSteps * TICKS_PER_STEP
  }

  private loops(): boolean {
    return this.state.mode === "pattern" || this.state.loopSong
  }

  /** Playback ran out by itself. */
  private finish() {
    this.state = { ...this.state, playing: false }
    this.position = 0
    this.tick = 0
  }

  /**
   * Brings a playhead that is past the end back inside the pattern or
   * song. A song with nothing on it has no inside, so it stops at once.
   */
  private keepInRange(project: Project) {
    if (!this.state.playing) return
    if (this.state.mode === "song" && this.region) {
      if (this.tick < this.region.start) this.tick = this.region.start
      if (this.tick >= this.region.end) {
        if (this.state.loopSong) this.tick = this.region.start
        else {
          this.tick = this.region.end
          this.position = this.tick
          this.state = { ...this.state, playing: false }
        }
      }
      return
    }
    const length = this.length(project)
    if (length <= 0) this.finish()
    else if (this.tick >= length) {
      if (this.loops()) this.tick %= length
      else this.finish()
    }
  }

  private step(project: Project, seconds: number): Onset[] {
    this.keepInRange(project)
    if (!this.state.playing) return []
    if (this.state.mode === "song") return this.stepSong(project, seconds)
    const length = this.length(project)
    const from = this.tick
    const to = from + (seconds * project.settings.tempoBpm * PPQ) / 60
    const pattern = project.patterns.find(
      (item) => item.id === this.state.pattern
    )
    const between = (low: number, high: number) =>
      this.state.mode === "song"
        ? songOnsets(project, low, high)
        : pattern
          ? patternOnsets(project, pattern, low, high)
          : []

    if (to < length) {
      this.tick = to
      return between(from, to)
    }
    const onsets = between(from, length)
    if (!this.loops()) {
      this.finish()
      return onsets
    }
    this.tick = (to - length) % length
    return [...onsets, ...between(0, this.tick)]
  }

  private stepSong(project: Project, seconds: number): Onset[] {
    const onsets: Onset[] = []
    const map = songTempoMap(project)
    let remaining = seconds
    for (let transitions = 0; ; transitions += 1) {
      const end = this.region?.end ?? songLength(project)
      let boundary = end
      let action: "pause" | "jump" | "end" = "end"
      let destination = this.region?.start ?? 0
      for (const marker of project.playlist.timeline?.markers ?? []) {
        const kind = marker.kind
        if (
          kind.type === "named" ||
          kind.type === "start" ||
          kind.type === "punch"
        )
          continue
        const at = kind.type === "loop" ? kind.end : marker.tick
        if (
          at < this.tick ||
          at > boundary ||
          (at === boundary && (!!this.region || action !== "end")) ||
          at === this.ignoredPause ||
          (this.region && at < this.region.start) ||
          (kind.type === "loop" && (this.region || !this.state.loopSong))
        )
          continue
        boundary = at
        action = kind.type === "pause" ? "pause" : "jump"
        destination =
          kind.type === "loop"
            ? marker.tick
            : kind.type === "skip"
              ? kind.end
              : at
      }
      const to = songPosition(project, this.tick, remaining)
      if (to < boundary) {
        onsets.push(...songOnsets(project, this.tick, to))
        this.tick = to
        break
      }
      onsets.push(...songOnsets(project, this.tick, boundary))
      remaining = Math.max(
        0,
        remaining - (map.secondsAt(boundary) - map.secondsAt(this.tick))
      )
      this.tick = boundary
      if (transitions >= 64) {
        this.navigationOverflows += 1
        this.position = boundary
        this.state = { ...this.state, playing: false }
        break
      }
      if (action === "pause" || (action === "end" && !this.state.loopSong)) {
        if (action === "end" && !this.region) this.finish()
        else {
          this.position = boundary
          this.state = { ...this.state, playing: false }
          this.ignoredPause = boundary
        }
        break
      }
      this.ignoredPause = null
      this.tick = this.region
        ? Math.min(destination, this.region.end)
        : destination
      if (remaining === 0) break
    }
    return onsets
  }

  /**
   * Two values per mixer track, in mixer order, the master first. A track
   * shows what plays into it, after its fader: its channels, plus what
   * other tracks pass on through their outputs and sends. Mute and solo
   * silence a track the way the engine's mixer does.
   */
  private meters(project: Project): number[] {
    const tracks = project.mixer.tracks
    const heard = soloSet(tracks)
    const outputs = new Map<number, number>()

    const outputOf = (id: number, depth: number): number => {
      const cached = outputs.get(id)
      if (cached !== undefined) return cached
      const track = tracks.find((item) => item.id === id)
      // Routing never loops. The depth limit is for a project that broke
      // that rule anyway.
      if (!track || depth > tracks.length) return 0
      let input = this.levels.get(id) ?? 0
      for (const other of tracks) {
        if (other.id === id || other.id === MASTER_TRACK) continue
        if (other.output === id) input += outputOf(other.id, depth + 1)
        for (const send of other.sends) {
          if (send.target === id) {
            input += outputOf(other.id, depth + 1) * send.gain
          }
        }
      }
      const silent = track.muted || !heard.has(id)
      const level = silent ? 0 : Math.min(METER_CEILING, input * track.volume)
      outputs.set(id, level)
      return level
    }

    return tracks.flatMap((track) => {
      const level = outputOf(track.id, 0)
      const left = level * Math.min(1, 1 - track.pan)
      const right = level * Math.min(1, 1 + track.pan)
      return [left, right * (0.94 + Math.random() * 0.06)]
    })
  }
}
