import { describe, expect, it } from "vitest"

import type { Command, Project } from "@/bindings"

import { buildProject, emptyProject } from "./project"
import { TransportSim } from "./transport"

/*
 * Three channels, each with a note on the first step and a mixer track of
 * its own: Drums (track 3), Bass (track 5) and Lead (track 7). Mixer order
 * is master, Drums, Bass, Lead, so the left meter of a track is at twice its
 * place in that list.
 */
function band(edit: (run: (command: Command) => number[]) => void = () => {}) {
  const project = buildProject(emptyProject(), (run) => {
    const channels = ["Drums", "Bass", "Lead"].map(
      (name) => run({ type: "addChannel", name })[0]
    )
    for (const channel of channels) {
      run({ type: "updateChannel", id: channel, patch: { volume: 1 } })
      run({
        type: "addNotes",
        pattern: 1,
        channel,
        notes: [{ start: 0, length: 240, key: 60, velocity: 1 }],
      })
    }
    edit(run)
  })
  expect(project.channels.map((channel) => channel.id)).toEqual(CHANNELS)
  expect(project.mixer.tracks.map((track) => track.id)).toEqual([
    0,
    DRUMS,
    BASS,
    LEAD,
  ])
  return project
}

const CHANNELS = [2, 4, 6]
const DRUMS = 3
const BASS = 5
const LEAD = 7

/** The left meter of each track just after the first step has sounded. */
function metersOf(project: Project) {
  const transport = new TransportSim(project.patterns[0].id)
  transport.play(project)
  const { meters } = transport.advance(project, 0.001)
  const left = (place: number) => Number(meters[place * 2].toFixed(3))
  return { master: left(0), drums: left(1), bass: left(2), lead: left(3) }
}

describe("simulated meters", () => {
  it("add up at the master what the tracks pass on", () => {
    expect(metersOf(band())).toEqual({
      master: 1.4,
      drums: 1,
      bass: 1,
      lead: 1,
    })
    const quiet = band((run) => {
      for (const id of [DRUMS, BASS, LEAD]) {
        run({ type: "updateMixerTrack", id, patch: { volume: 0.25 } })
      }
    })
    expect(metersOf(quiet)).toEqual({
      master: 0.75,
      drums: 0.25,
      bass: 0.25,
      lead: 0.25,
    })
  })

  it("follow a send, at the level of the send", () => {
    const project = band((run) => {
      // The lead track makes no sound of its own, and hears half the drums.
      run({ type: "updateChannel", id: CHANNELS[2], patch: { muted: true } })
      run({ type: "setSend", from: DRUMS, to: LEAD, gain: 0.5 })
      run({ type: "updateMixerTrack", id: BASS, patch: { muted: true } })
    })
    expect(metersOf(project)).toEqual({
      master: 1.4,
      drums: 1,
      bass: 0,
      lead: 0.5,
    })
  })

  it("follow a track with no output only through its sends", () => {
    const project = band((run) => {
      run({ type: "setTrackOutput", id: DRUMS })
      run({ type: "updateMixerTrack", id: BASS, patch: { muted: true } })
      run({ type: "updateMixerTrack", id: LEAD, patch: { muted: true } })
    })
    expect(metersOf(project)).toMatchObject({ master: 0, drums: 1 })
    const sent = buildProject(project, (run) => {
      run({ type: "setSend", from: DRUMS, to: 0, gain: 0.3 })
    })
    expect(metersOf(sent)).toMatchObject({ master: 0.3, drums: 1 })
  })

  it("keep what a soloed track feeds and is fed by, as the engine does", () => {
    // Drums plays into Lead, which is soloed: both are heard, Bass is not.
    const bus = band((run) => {
      run({ type: "setTrackOutput", id: DRUMS, output: LEAD })
      run({ type: "updateMixerTrack", id: LEAD, patch: { solo: true } })
      for (const id of [DRUMS, BASS, LEAD]) {
        run({ type: "updateMixerTrack", id, patch: { volume: 0.5 } })
      }
    })
    expect(metersOf(bus)).toEqual({
      // Lead carries itself and half the drums: (1 + 0.5) / 2.
      master: 0.75,
      drums: 0.5,
      bass: 0,
      lead: 0.75,
    })

    // Soloing the source instead keeps the bus it plays through.
    const source = buildProject(bus, (run) => {
      run({ type: "updateMixerTrack", id: LEAD, patch: { solo: false } })
      run({ type: "updateMixerTrack", id: DRUMS, patch: { solo: true } })
    })
    expect(metersOf(source)).toMatchObject({ drums: 0.5, bass: 0, lead: 0.75 })

    // Mute wins over solo.
    const muted = buildProject(source, (run) => {
      run({ type: "updateMixerTrack", id: DRUMS, patch: { muted: true } })
    })
    expect(metersOf(muted)).toMatchObject({ drums: 0, bass: 0, lead: 0.5 })
  })

  it("leave out a channel that is muted or outside the solo", () => {
    const project = band((run) => {
      run({ type: "updateChannel", id: CHANNELS[0], patch: { solo: true } })
    })
    expect(metersOf(project)).toMatchObject({ drums: 1, bass: 0, lead: 0 })
  })
})

describe("simulated transport", () => {
  /** A project at 120 bpm, where a second is 1920 ticks. */
  function song(clipLength?: number) {
    return buildProject(emptyProject(), (run) => {
      if (clipLength === undefined) return
      const [track] = run({ type: "addPlaylistTrack" })
      run({
        type: "addClips",
        clips: [
          {
            track,
            start: 0,
            length: clipLength,
            content: { type: "pattern", pattern: 1 },
          },
        ],
      })
    })
  }

  it("uses imported start anchors and passes punch markers without navigation", () => {
    const project = song(1920)
    project.playlist.timeline = {
      meters: [],
      markers: [
        {
          id: project.nextId++,
          tick: 240,
          name: "Start",
          kind: { type: "start" },
        },
        {
          id: project.nextId++,
          tick: 480,
          name: "Punch",
          kind: { type: "punch", end: 960 },
        },
      ],
    }
    const transport = new TransportSim(1)
    transport.set({ mode: "song" }, project)
    transport.play(project)
    expect(transport.tick).toBe(240)
    expect(transport.advance(project, 0.5).playing).toBe(true)
    expect(transport.tick).toBe(1200)
    transport.stop()
    transport.seek(600, project)
    transport.play(project)
    transport.advance(project, 0.125)
    expect(transport.tick).toBe(840)
  })

  it("stops where playback started, and starts there again", () => {
    const project = song()
    const transport = new TransportSim(1)
    transport.seek(480, project)
    transport.play(project)
    transport.advance(project, 0.5)
    expect(transport.tick).toBe(1440)

    transport.stop()
    expect(transport.tick).toBe(480)
    transport.play(project)
    expect(transport.advance(project, 0.25).tick).toBe(960)

    // A seek while playing moves the place to return to as well.
    transport.seek(2400, project)
    transport.advance(project, 0.25)
    transport.stop()
    expect(transport.tick).toBe(2400)
  })

  it("loops a pattern and wraps a playhead that is past its end", () => {
    const project = song()
    const transport = new TransportSim(1)
    transport.play(project)
    transport.advance(project, 2.5)
    // The pattern is 3840 ticks long: two and a half seconds is 4800.
    expect(transport.tick).toBe(960)
    transport.seek(3840 + 100, project)
    expect(transport.tick).toBe(100)
  })

  it("stops an empty song at once", () => {
    const project = song()
    const transport = new TransportSim(1, true)
    transport.set({ mode: "song" }, project)
    transport.play(project)
    expect(transport.state.playing).toBe(false)
    expect(transport.advance(project, 0.1)).toMatchObject({
      playing: false,
      tick: 0,
    })

    // Changing to an empty song while a pattern plays ends playback too.
    transport.set({ mode: "pattern" }, project)
    transport.play(project)
    expect(transport.state.playing).toBe(true)
    transport.set({ mode: "song" }, project)
    expect(transport.state.playing).toBe(false)
  })

  it("ends a song after its last clip unless it loops, and rewinds", () => {
    const project = song(1920)
    const transport = new TransportSim(1)
    transport.set({ mode: "song" }, project)
    transport.seek(960, project)
    transport.play(project)
    transport.advance(project, 0.75)
    expect(transport.state.playing).toBe(false)
    // Playback that ran out starts from the top next time.
    expect(transport.tick).toBe(0)

    transport.set({ loopSong: true }, project)
    transport.play(project)
    transport.advance(project, 1.25)
    expect(transport.state.playing).toBe(true)
    expect(transport.tick).toBe(480)
  })

  it("starts over when the mode changes, and not when the pattern does", () => {
    const project = buildProject(song(7680), (run) => {
      run({ type: "addPattern" })
    })
    const other = project.patterns[1].id
    const transport = new TransportSim(1)
    transport.play(project)
    transport.advance(project, 0.5)
    transport.set({ pattern: other }, project)
    expect(transport.tick).toBe(960)
    transport.set({ mode: "song" }, project)
    expect(transport.tick).toBe(0)
    transport.stop()
    expect(transport.tick).toBe(0)
  })

  it("mirrors skip, pause/resume, selected stop/loop precedence and bounded tiny loops", () => {
    const project = buildProject(song(1920), (run) => {
      run({
        type: "addTimelineMarker",
        tick: 13,
        name: "Skip",
        kind: { type: "skip", end: 31 },
      })
      run({
        type: "addTimelineMarker",
        tick: 53,
        name: "Pause",
        kind: { type: "pause" },
      })
      run({
        type: "addTimelineMarker",
        tick: 100,
        name: "Loop",
        kind: { type: "loop", end: 150 },
      })
    })
    const transport = new TransportSim(1)
    transport.set({ mode: "song" }, project)
    transport.play(project)
    transport.advance(project, 35 / 1920)
    expect(transport.tick).toBe(53)
    expect(transport.state.playing).toBe(false)
    transport.play(project)
    transport.advance(project, 7 / 1920)
    expect(transport.tick).toBeCloseTo(60)
    expect(transport.state.playing).toBe(true)
    transport.seek(53, project)
    transport.advance(project, 1 / 1920)
    expect(transport.state.playing).toBe(false)
    transport.setRegion({ start: 161, end: 177 }, project)
    transport.seek(0, project)
    transport.play(project)
    transport.advance(project, 17 / 1920)
    expect(transport.tick).toBe(177)
    expect(transport.state.playing).toBe(false)
    transport.set({ loopSong: true }, project)
    transport.play(project)
    transport.advance(project, 19 / 1920)
    expect(transport.tick).toBeCloseTo(164)
    transport.setRegion({ start: 1, end: 2 }, project)
    transport.advance(project, 100 / 1920)
    expect(transport.state.playing).toBe(false)
    expect(transport.navigationOverflows).toBe(1)
  })

  it("pauses at the natural song end while an explicit range end wins equal boundaries", () => {
    const project = buildProject(song(100), (run) => {
      run({
        type: "addTimelineMarker",
        tick: 100,
        name: "End pause",
        kind: { type: "pause" },
      })
    })
    const transport = new TransportSim(1)
    transport.set({ mode: "song" }, project)
    transport.play(project)
    transport.advance(project, 101 / 1920)
    expect(transport.tick).toBe(100)
    expect(transport.state.playing).toBe(false)
    transport.setRegion({ start: 90, end: 100 }, project)
    transport.set({ loopSong: true }, project)
    transport.play(project)
    transport.advance(project, 13 / 1920)
    expect(transport.tick).toBeCloseTo(93)
    expect(transport.state.playing).toBe(true)
  })
})
