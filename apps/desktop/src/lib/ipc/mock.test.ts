import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type {
  Project,
  ProjectPatch,
  RealtimeFrame,
  TransportState,
} from "@/bindings"
import { sourceSample } from "@/lib/channel-source"
import { TEST_DIALOGS } from "@/test/harness"

import { errorMessage, errorSource } from "./backend"
import { createMockBackend } from "./mock"
import { demoProject } from "./sim/project"

function create() {
  return createMockBackend({ storage: null, dialogs: TEST_DIALOGS })
}

describe("mock backend: document", () => {
  it("preserves reserved native messages through string and Error refusals", () => {
    const source = 'windfall-message:{"code":"midi.import.markersNotImported","params":{"count":2}}'
    // What is kept is the source: the code and its parameters, unchanged.
    expect(errorSource(source)).toBe(source)
    expect(errorSource(new Error(source))).toBe(source)
    const malformed = "windfall-message:{"
    expect(errorSource(malformed)).toBe(malformed)
    // What is shown now is its words in the interface language.
    expect(errorMessage(source)).toBe("2 markers not imported")
    expect(errorMessage(new Error(source))).toBe("2 markers not imported")
    expect(errorMessage(malformed)).toBe("The native message could not be decoded.")
  })

  it("starts with the demo project, clean and unsaved", async () => {
    const snapshot = await create().documentSnapshot()
    expect(snapshot).toMatchObject({ revision: 0, dirty: false, path: null })
    expect(snapshot.project.channels).toHaveLength(4)
  })

  it("emits one patch per edit with revisions that count up", async () => {
    const backend = create()
    const seen: ProjectPatch[] = []
    backend.onProjectPatch((patch) => seen.push(patch))

    const first = await backend.dispatch({ type: "addPattern" })
    const second = await backend.dispatch({ type: "addChannel" })
    await backend.undo()

    expect(seen.map((patch) => patch.revision)).toEqual([1, 2, 3])
    expect(seen[0]).toEqual(first.patch)
    expect(second.created).toHaveLength(2)
    expect(seen[2].history.cursor).toBe(1)
    expect(seen[2].channels).toHaveLength(4)
  })

  it("rejects a bad command with a plain message and emits nothing", async () => {
    const backend = create()
    const onPatch = vi.fn()
    backend.onProjectPatch(onPatch)

    const refused = backend.dispatch({ type: "removeChannel", id: 12345 })
    // The document's words, as the shell passes them on.
    await expect(refused).rejects.toThrow(
      new Error("channel 12345 does not exist")
    )
    // What the user is shown starts with a capital.
    expect(errorMessage(await refused.catch((error: unknown) => error))).toBe(
      "Channel 12345 does not exist"
    )
    expect(errorMessage("already a sentence.")).toBe("Already a sentence.")
    expect(onPatch).not.toHaveBeenCalled()
    expect((await backend.documentSnapshot()).revision).toBe(0)
  })

  it("resolves undo and redo to null at the ends of the history", async () => {
    const backend = create()
    expect(await backend.undo()).toBeNull()
    await backend.dispatch({ type: "addPattern" })
    expect(await backend.redo()).toBeNull()
    expect((await backend.historyJump(0)).history.cursor).toBe(0)
  })

  it("adds a channel from a file as one undo step", async () => {
    const backend = create()
    const result = await backend.addChannelFromFile(
      "/factory/Drums/Percussion/Rim 01.wav",
      0
    )
    const { project, history } = await backend.documentSnapshot()

    expect(history.entries).toEqual([{ label: "Add channel" }])
    expect(project.channels[0]).toMatchObject({ name: "Rim 01" })
    expect(project.samples.at(-1)).toMatchObject({
      id: sourceSample(project.channels[0].source),
      path: { kind: "factory", path: "Drums/Percussion/Rim 01.wav" },
    })
    expect(result.created).toHaveLength(3)

    await backend.undo()
    const undone = await backend.documentSnapshot()
    expect(undone.project.channels).toHaveLength(4)
    expect(undone.project.samples).toHaveLength(4)
  })

  it("applies the document's own rules, not rules of its own", async () => {
    const backend = create()
    const { project } = await backend.documentSnapshot()
    const kick = project.channels[0]

    // A number outside its range is brought into it.
    await backend.dispatch({
      type: "updateChannel",
      id: kick.id,
      patch: { volume: 9, pan: -4 },
    })
    await backend.dispatch({ type: "updateSettings", patch: { tempoBpm: 1 } })
    const clamped = await backend.documentSnapshot()
    expect(clamped.project.channels[0]).toMatchObject({ volume: 2, pan: -1 })
    expect(clamped.project.settings.tempoBpm).toBe(10)
    expect(clamped.history.entries).toEqual([
      { label: "Change channel" },
      { label: "Change tempo" },
    ])

    // An edit that changes nothing leaves no undo step, and neither does a
    // drag that ends where it started.
    await backend.dispatch({ type: "updateSettings", patch: { tempoBpm: 10 } })
    await backend.dispatch({ type: "updateSettings", patch: { swing: 0.5 } }, 7)
    await backend.dispatch({ type: "updateSettings", patch: { swing: 0 } }, 7)
    await backend.dispatch({ type: "batch", commands: [] })
    expect((await backend.documentSnapshot()).history.entries).toHaveLength(2)

    // Names, colors and where things go are the document's.
    const copy = await backend.dispatch({
      type: "duplicateChannel",
      id: kick.id,
    })
    const after = (await backend.documentSnapshot()).project
    expect(after.channels[1]).toMatchObject({
      id: copy.created[0],
      name: "Kick #2",
      mixerTrack: kick.mixerTrack,
    })
    for (const pattern of after.patterns) {
      const lanes = pattern.lanes.map((lane) => lane.channel)
      expect(lanes).toEqual([...lanes].sort((a, b) => a - b))
    }
    await expect(
      backend.dispatch({ type: "removePattern", id: project.patterns[0].id })
    ).rejects.toThrow("a project needs at least one pattern")
  })

  it("refuses a project that breaks a rule of the model", () => {
    const project = demoProject()
    const lanes = [...project.patterns[0].lanes].reverse()
    const unsorted = {
      ...project,
      patterns: [{ ...project.patterns[0], lanes }],
    }
    expect(() =>
      createMockBackend({ storage: null, project: unsorted })
    ).toThrow("the project breaks a rule of the project model")
  })

  it("rejects every call once it is disposed", async () => {
    const backend = create()
    await backend.dispatch({ type: "addPattern" })
    backend.dispose()
    backend.dispose()
    await expect(backend.dispatch({ type: "addPattern" })).rejects.toThrow(
      "This document was closed."
    )
  })

  it("reuses a sample that is already in the pool", async () => {
    const backend = create()
    const before = await backend.documentSnapshot()
    const channel = before.project.channels[1].id
    await backend.setChannelSampleFromFile(
      channel,
      "/factory/Drums/Kicks/Kick 01.wav"
    )
    const after = await backend.documentSnapshot()
    expect(after.project.samples).toHaveLength(4)
    expect(sourceSample(after.project.channels[1].source)).toBe(
      sourceSample(before.project.channels[0].source)
    )
    await expect(
      backend.addChannelFromFile("/factory/Drums/Notes.txt")
    ).rejects.toThrow("is not an audio file")
  })
})

describe("mock backend: files", () => {
  it("has no path until the project is saved, then remembers it", async () => {
    const backend = create()
    await backend.dispatch({ type: "addPattern" })
    await expect(backend.projectSave()).rejects.toThrow(
      "This project has no file yet"
    )

    expect(await backend.projectSave("/projects/beat.windfall")).toBe(
      "/projects/beat.windfall"
    )
    expect(await backend.documentSnapshot()).toMatchObject({
      dirty: false,
      path: "/projects/beat.windfall",
    })
    expect(await backend.recentProjects()).toEqual(["/projects/beat.windfall"])
    expect(await backend.projectSave()).toBe("/projects/beat.windfall")
    // A name without the extension gets it.
    expect(await backend.projectSave("/projects/copy")).toBe(
      "/projects/copy.windfall"
    )
  })

  it("opens what was saved and announces the load", async () => {
    const backend = create()
    const loaded = vi.fn()
    backend.onProjectLoaded(loaded)
    await backend.dispatch({
      type: "updateSettings",
      patch: { name: "Night drive", tempoBpm: 90 },
    })
    await backend.projectSave("/projects/night.windfall")

    const fresh = await backend.projectNew()
    expect(fresh).toMatchObject({ revision: 0, path: null, dirty: false })
    expect(fresh.project.settings).toMatchObject({
      name: "Untitled",
      tempoBpm: 120,
    })

    const opened = await backend.projectOpen("/projects/night.windfall")
    expect(opened.project.settings).toMatchObject({
      name: "Night drive",
      tempoBpm: 90,
    })
    expect(opened.history.entries).toHaveLength(0)
    expect(loaded).toHaveBeenCalledTimes(2)
    await expect(backend.projectOpen("/nowhere.windfall")).rejects.toThrow(
      "Could not read /nowhere.windfall: no such file is saved in this browser."
    )
  })

  it("starts a new project with the app's starter kit, outside the history", async () => {
    const backend = create()
    const warnings = vi.fn()
    backend.onProjectWarnings(warnings)
    const { project, history } = await backend.projectNew()

    // The same sounds and volumes as `default_project` in the shell.
    expect(
      project.channels.map((channel) => {
        const sample = project.samples.find(
          (item) => item.id === sourceSample(channel.source)
        )
        return [channel.name, channel.volume, sample?.path]
      })
    ).toEqual([
      [
        "Kick Punch",
        0.36,
        { kind: "factory", path: "Drums/Kicks/Kick Punch.wav" },
      ],
      [
        "Clap Wide",
        0.25,
        { kind: "factory", path: "Drums/Claps/Clap Wide.wav" },
      ],
      [
        "Hat Closed 1",
        0.18,
        { kind: "factory", path: "Drums/Hats/Hat Closed 1.wav" },
      ],
      [
        "Snare Tight",
        0.29,
        { kind: "factory", path: "Drums/Snares/Snare Tight.wav" },
      ],
    ])
    // One mixer track each, beside the master.
    expect(project.mixer.tracks).toHaveLength(5)
    expect(
      new Set(project.channels.map((channel) => channel.mixerTrack)).size
    ).toBe(4)
    expect(history.entries).toEqual([])
    expect(await backend.undo()).toBeNull()
    expect(warnings).not.toHaveBeenCalled()
  })

  it("saves the real file format and loads it with the real checks", async () => {
    const storage = new Map<string, string>()
    const backend = createMockBackend({
      dialogs: TEST_DIALOGS,
      storage: {
        getItem: (key) => storage.get(key) ?? null,
        setItem: (key, value) => void storage.set(key, value),
      },
    })
    await backend.projectSave("/projects/real.windfall")
    const files = JSON.parse(
      storage.get("windfall.mock.project-files") ?? "{}"
    ) as Record<string, string>
    const saved = files["/projects/real.windfall"]
    // Pretty-printed JSON that ends in a newline, as on disk.
    expect(saved.startsWith('{\n  "formatVersion": 1,')).toBe(true)
    expect(saved.endsWith("}\n")).toBe(true)
    const project = JSON.parse(saved) as Project
    expect(project).toEqual((await backend.documentSnapshot()).project)

    const put = (path: string, text: string) =>
      storage.set(
        "windfall.mock.project-files",
        JSON.stringify({ ...files, [path]: text })
      )
    put("/projects/garbage.windfall", "not a project")
    await expect(
      backend.projectOpen("/projects/garbage.windfall")
    ).rejects.toThrow(/^This is not a Windfall project: /)

    put(
      "/projects/newer.windfall",
      JSON.stringify({ ...project, formatVersion: 99 })
    )
    await expect(
      backend.projectOpen("/projects/newer.windfall")
    ).rejects.toThrow(
      "This project was saved by a newer version of Windfall (file format 99; this version reads up to format 1)"
    )

    put("/projects/damaged.windfall", JSON.stringify({ ...project, nextId: 1 }))
    await expect(
      backend.projectOpen("/projects/damaged.windfall")
    ).rejects.toThrow(/^The project file is damaged: /)

    // None of them replaced the open project.
    expect((await backend.documentSnapshot()).path).toBe(
      "/projects/real.windfall"
    )
  })

  it("keeps saved projects in the storage it is given", async () => {
    const backend = createMockBackend({ dialogs: TEST_DIALOGS })
    await backend.projectSave("/projects/kept.windfall")
    const later = createMockBackend({ dialogs: TEST_DIALOGS })
    expect(await later.recentProjects()).toEqual(["/projects/kept.windfall"])
    expect((await later.projectOpen("/projects/kept.windfall")).path).toBe(
      "/projects/kept.windfall"
    )
  })
})

describe("mock backend: browser and engine", () => {
  it("lists folders before files and describes samples", async () => {
    const backend = create()
    const [factory] = await backend.browserRoots()
    expect(factory).toMatchObject({ kind: "factory" })

    const top = await backend.browserList(factory.path)
    expect(top.map((entry) => entry.kind)).toEqual(["folder", "folder"])
    const hats = await backend.browserList(`${factory.path}/Drums/Hats`)
    expect(hats.map((entry) => entry.name)).toEqual([
      "Closed Hat 01.wav",
      "Closed Hat 02.wav",
      "Hat Closed 1.wav",
      "Open Hat 01.wav",
    ])

    const info = await backend.sampleInfo(hats[0].path)
    expect(info.name).toBe("Closed Hat 01")
    expect(info.peaks.length % 2).toBe(0)
    expect(info.frames).toBe(Math.round(info.durationSecs * info.sampleRate))
    await expect(backend.sampleInfo("/factory/Drums")).rejects.toThrow()
  })

  it("adds and removes user folders but not the factory library", async () => {
    const backend = create()
    const roots = await backend.browserAddRoot("/samples/Mine/")
    expect(roots.at(-1)).toEqual({
      name: "Mine",
      path: "/samples/Mine",
      kind: "user",
    })
    await expect(backend.browserAddRoot("/samples/Mine")).rejects.toThrow(
      "already in the browser"
    )
    expect(await backend.browserRemoveRoot("/samples/Mine")).toHaveLength(1)
    await expect(backend.browserRemoveRoot("/factory")).rejects.toThrow(
      "cannot be removed"
    )
  })

  it("reports an engine error for a device that does not exist", async () => {
    const backend = create()
    const onStatus = vi.fn()
    backend.onEngineStatus(onStatus)

    expect(await backend.engineStatus()).toMatchObject({
      running: true,
      host: "WASAPI",
    })
    const failed = await backend.engineConfigure({ device: "Unplugged" })
    expect(failed.running).toBe(false)
    expect(failed.error).toContain("Unplugged")
    expect(onStatus).toHaveBeenCalledWith(failed)

    const hosts = await backend.engineDevices()
    const working = await backend.engineConfigure({
      host: hosts[1].name,
      bufferFrames: 128,
      sampleRate: 96_000,
    })
    expect(working).toMatchObject({ running: true, bufferFrames: 128 })
    expect(working.latencyMs).toBeCloseTo(1.333, 2)
  })

  it("keeps what was asked for apart from what the engine runs at", async () => {
    const backend = create()
    // The shell writes a field left to the system as null, and so does this.
    expect(await backend.engineSettings()).toStrictEqual({
      host: null,
      device: null,
      sampleRate: null,
      bufferFrames: null,
      outputChannels: null,
    })
    await backend.engineConfigure({ sampleRate: 44_100 })
    // The buffer was left to the device, and stays "default" in the request.
    expect(await backend.engineSettings()).toStrictEqual({
      host: null,
      device: null,
      sampleRate: 44_100,
      bufferFrames: null,
      outputChannels: null,
    })
    expect(await backend.engineStatus()).toMatchObject({
      sampleRate: 44_100,
      bufferFrames: 256,
    })
  })

  it("picks an audio file through the dialog it is given", async () => {
    expect(await create().pickAudioFile()).toBe(
      "/factory/Drums/Kicks/Kick 02.wav"
    )
  })

  it("warns about samples whose files are gone, on load and on reload", async () => {
    const backend = create()
    const warnings: string[][] = []
    backend.onProjectWarnings((list) => warnings.push(list))

    // A sample from a folder that is in the browser, saved with the project.
    await backend.browserAddRoot("/samples/Mine")
    await backend.addChannelFromFile("/samples/Mine/Vocal chop.wav")
    await backend.projectSave("/projects/song.windfall")
    await backend.projectOpen("/projects/song.windfall")
    expect(warnings).toEqual([])

    // The folder goes away, and with it the file.
    await backend.browserRemoveRoot("/samples/Mine")
    await backend.samplesReload()
    expect(warnings).toHaveLength(1)
    expect(warnings[0][0]).toContain('The sample "Vocal chop" is missing')
    const sample = (await backend.documentSnapshot()).project.samples.at(-1)
    if (!sample) throw new Error("the sample is gone")
    await expect(backend.sampleInfoById(sample.id)).rejects.toThrow()

    await backend.projectOpen("/projects/song.windfall")
    expect(warnings).toHaveLength(2)

    // Back again: a reload reports nothing missing.
    await backend.browserAddRoot("/samples/Mine")
    await backend.samplesReload()
    expect(warnings.at(-1)).toEqual([])
  })
})

describe("mock backend: transport", () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  const flush = () => vi.advanceTimersByTimeAsync(0)

  it("moves the playhead with the tempo and loops the pattern", async () => {
    const backend = create()
    const frames: RealtimeFrame[] = []
    const stopFrames = backend.subscribeRealtime((frame) => frames.push(frame))

    await vi.advanceTimersByTimeAsync(100)
    expect(frames.at(-1)).toMatchObject({ playing: false, tick: 0 })

    void backend.transportPlay()
    await vi.advanceTimersByTimeAsync(1000)
    const playing = frames.at(-1)
    // 128 BPM is 2048 ticks a second.
    expect(playing?.tick).toBeGreaterThan(1800)
    expect(playing?.tick).toBeLessThan(2200)
    expect(playing?.meters).toHaveLength(10)
    expect(
      Math.max(...frames.flatMap((frame) => frame.meters))
    ).toBeGreaterThan(0.3)

    await vi.advanceTimersByTimeAsync(1500)
    expect(frames.at(-1)?.tick).toBeLessThan(3840)

    void backend.transportStop()
    await vi.advanceTimersByTimeAsync(50)
    expect(frames.at(-1)).toMatchObject({ playing: false, tick: 0 })
    stopFrames()
  })

  it("returns the playhead to where playback started when it stops", async () => {
    const backend = create()
    const frames: RealtimeFrame[] = []
    const stopFrames = backend.subscribeRealtime((frame) => frames.push(frame))

    void backend.transportSeek(960)
    void backend.transportPlay()
    await vi.advanceTimersByTimeAsync(500)
    expect(frames.at(-1)?.tick).toBeGreaterThan(960)
    void backend.transportStop()
    await vi.advanceTimersByTimeAsync(50)
    expect(frames.at(-1)).toMatchObject({ playing: false, tick: 960 })

    // The next play starts there again.
    void backend.transportPlay()
    await vi.advanceTimersByTimeAsync(20)
    expect(frames.at(-1)?.tick).toBeGreaterThan(960)
    expect(frames.at(-1)?.tick).toBeLessThan(1200)
    stopFrames()
  })

  it("announces every transport change", async () => {
    const backend = create()
    const states: TransportState[] = []
    backend.onTransportState((state) => states.push(state))

    void backend.transportToggle()
    void backend.transportSet({ mode: "pattern", loopSong: true })
    void backend.transportToggle()
    // A song with nothing on it has nothing to play, so it stops at once.
    void backend.transportToggle()
    void backend.transportSet({ mode: "song" })
    await flush()

    expect(
      states.map((state) => [state.playing, state.mode, state.loopSong])
    ).toEqual([
      [true, "pattern", false],
      [true, "pattern", true],
      [false, "pattern", true],
      [true, "pattern", true],
      [false, "song", true],
    ])
    states.length = 0

    // An empty song has nothing to play, and the transport says so.
    await expect(backend.transportToggle()).rejects.toThrow(
      "The playlist is empty"
    )
    await expect(backend.transportPlay()).rejects.toThrow(
      "The playlist is empty"
    )
    expect(states).toHaveLength(0)
    expect((await backend.transportState()).playing).toBe(false)

    // Nor has a song whose every clip is muted.
    const { project } = await backend.documentSnapshot()
    const track = await backend.dispatch({ type: "addPlaylistTrack" })
    const clip = await backend.dispatch({
      type: "addClips",
      clips: [
        {
          track: track.created[0],
          start: 0,
          content: { type: "pattern", pattern: project.patterns[0].id },
        },
      ],
    })
    await backend.dispatch({
      type: "updateClips",
      updates: [{ id: clip.created[0], patch: { muted: true } }],
    })
    await expect(backend.transportPlay()).rejects.toThrow(
      "Every clip on the playlist is muted"
    )
    await backend.dispatch({
      type: "updateClips",
      updates: [{ id: clip.created[0], patch: { muted: false } }],
    })
    expect((await backend.transportPlay()).playing).toBe(true)

    // The song ends with its last clip, so removing it ends playback.
    const frames: RealtimeFrame[] = []
    const stopFrames = backend.subscribeRealtime((frame) => frames.push(frame))
    await backend.dispatch({ type: "removeClips", clips: [clip.created[0]] })
    await vi.advanceTimersByTimeAsync(50)
    expect(frames.at(-1)).toMatchObject({ playing: false, tick: 0 })
    expect(states.at(-1)?.playing).toBe(false)
    stopFrames()
    await expect(backend.transportSet({ pattern: 999 })).rejects.toThrow(
      "pattern 999 does not exist"
    )
  })

  it("moves the transport off a pattern that is deleted", async () => {
    const backend = create()
    const states: TransportState[] = []
    backend.onTransportState((state) => states.push(state))
    const added = backend.dispatch({ type: "addPattern" })
    await flush()
    const id = (await added).created[0]

    void backend.transportSet({ pattern: id })
    void backend.dispatch({ type: "removePattern", id })
    await flush()
    expect(states.at(-1)?.pattern).toBe(1)
  })

  it("reports export progress until it is done", async () => {
    const backend = create()
    const fractions: number[] = []
    const done = vi.fn()
    backend.onExportProgress((progress) => {
      fractions.push(progress.fraction)
      if (progress.done) done(progress.path)
    })
    void backend.exportAudio({
      path: "/exports/beat.wav",
      format: "wav",
      bitDepth: "int24",
      sampleRate: 48_000,
      mode: "pattern",
      patternLoops: 2,
      tailSecs: 1,
      autoTail: false,
    })
    await vi.advanceTimersByTimeAsync(3000)

    expect(done).toHaveBeenCalledExactlyOnceWith("/exports/beat.wav")
    expect(fractions.at(-1)).toBe(1)
    expect([...fractions].sort((a, b) => a - b)).toEqual(fractions)
  })
})
