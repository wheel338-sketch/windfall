import { describe, expect, it } from "vitest"

// The source of the generated binding, to read the list of commands from.
import commandBinding from "@/bindings/Command.ts?raw"
import type {
  Command,
  EffectParams,
  InstrumentParams,
  Project,
} from "@/bindings"
import descriptors from "@/bindings/descriptors.json"
import { createDefaultChannelVoiceSettings } from "@/features/channel-rack/voice/settings"
import { defaultAsset } from "@/features/sample-playback/settings"
import { DEFAULT_TRACK_PROCESSING } from "@/lib/track-processing"

import { SimDocument } from "./document"
import { buildProject, demoProject, emptyProject } from "./project"

/** A project with something of every kind in it, and the ids of the parts. */
function fullProject() {
  const ids = {
    sample: 0,
    spareSample: 0,
    channel: 0,
    synth: 0,
    keyDrum: 0,
    slicer: 0,
    sampleInstrument: 0,
    effect: 0,
    tap: 0,
    convolver: 0,
    sparePattern: 0,
    track: 0,
    playlistTrack: 0,
    lowerTrack: 0,
    clip: 0,
    audioClip: 0,
    automation: 0,
    meter: 0,
    marker: 0,
  }
  const base = demoProject()
  const project = buildProject(base, (run) => {
    ;[ids.meter] = run({
      type: "addMeterChange",
      tick: 4001,
      signature: { numerator: 7, denominator: 8 },
    })
    ;[ids.marker] = run({
      type: "addTimelineMarker",
      tick: 7400,
      name: "Verse",
      kind: { type: "named" },
    })
    ids.sample = base.samples[0].id
    ids.channel = base.channels[0].id
    ;[ids.spareSample] = run({
      type: "addSample",
      name: "Spare",
      path: { kind: "factory", path: "Drums/Kicks/Kick 02.wav" },
    })
    ;[ids.sparePattern] = run({ type: "addPattern" })
    ;[ids.track] = run({ type: "addMixerTrack" })
    ;[ids.synth] = run({ type: "addChannel", instrument: "subtractiveSynth" })
    ;[ids.keyDrum] = run({ type: "addChannel", instrument: "drumVoice" })
    ;[ids.slicer] = run({ type: "addChannel", instrument: "sliceDeck" })
    run({ type: "setSamplePlayback", channel: ids.slicer, asset: { ...defaultAsset(), sample: ids.sample } })
    ;[ids.sampleInstrument] = run({ type: "addChannel", instrument: "multiSampler" })
    ;[ids.effect] = run({
      type: "addEffect",
      track: ids.track,
      kind: "compressor",
    })
    run({ type: "addEffect", track: ids.track, kind: "delay" })
    ;[ids.convolver] = run({ type: "addEffect", track: ids.track, kind: "convolver" })
    ;[ids.tap] = run({ type: "addEffect", track: ids.track, kind: "sendTap" })
    ;[ids.playlistTrack] = run({ type: "addPlaylistTrack" })
    ;[ids.clip] = run({
      type: "addClips",
      clips: [
        {
          track: ids.playlistTrack,
          start: 0,
          content: { type: "pattern", pattern: base.patterns[0].id },
        },
      ],
    })
    ;[ids.lowerTrack] = run({ type: "addPlaylistTrack" })
    ;[ids.audioClip] = run({
      type: "addClips",
      clips: [
        {
          track: ids.lowerTrack,
          start: 0,
          length: 1920,
          content: {
            type: "audio",
            sample: ids.sample,
            mixerTrack: ids.track,
            gain: 1,
            pan: 0,
            fadeIn: 0,
            fadeOut: 0,
            reverse: false,
            pitch: 0,
          },
        },
      ],
    })
    ;[ids.automation] = run({
      type: "addAutomation",
      target: { type: "trackVolume", track: ids.track },
    })
    run({
      type: "addClips",
      clips: [
        {
          track: ids.lowerTrack,
          start: 3840,
          content: { type: "automation", automation: ids.automation },
        },
      ],
    })
  })
  const scratch = SimDocument.create(project)
  const [pluginChannel] = scratch.dispatch({
    type: "addPluginInstrument",
    plugin: pluginBinding(),
  }).created
  const [pluginEffect] = scratch.dispatch({
    type: "addPluginEffect",
    track: ids.track,
    plugin: pluginBinding(),
  }).created
  scratch.dispatch({
    type: "addArrangement",
    name: "Original",
    clips: [],
    tracks: [],
  })
  const [arrangement] = scratch.dispatch({
    type: "addArrangement",
    name: "Alternate",
    clips: [],
    tracks: [],
  }).created
  const [group] = scratch.dispatch({
    type: "addTrackGroup",
    name: "Drums",
    parent: null,
  }).created
  const [parentGroup] = scratch.dispatch({
    type: "addTrackGroup",
    name: "Band",
    parent: null,
  }).created
  const [clipGroup] = scratch.dispatch({
    type: "addClipGroup",
    clips: [ids.clip, ids.audioClip],
  }).created
  scratch.dispatch({
    type: "setChannelGroup",
    channels: [ids.channel],
    group: "Drums",
  })
  const sourceAudio = scratch
    .project()
    .playlist.clips.find((clip) => clip.id === ids.audioClip)!
  const [spareAudioClip] = scratch.dispatch({
    type: "addClips",
    clips: [
      {
        track: ids.lowerTrack,
        start: 1920,
        length: 1920,
        content: sourceAudio.content,
      },
    ],
  }).created
  const [takeGroup] = scratch.dispatch({
    type: "createAudioTakeGroup",
    name: "Takes",
    lanes: [{ name: "Mic", takes: [{ pass: 1, clip: ids.audioClip }] }],
  }).created
  scratch.dispatch({
    type: "updateClips",
    updates: [{ id: ids.audioClip, patch: { muted: true } }],
  })
  const withPlugin = scratch.project()
  scratch.dispose()
  const pattern = withPlugin.patterns[0]
  const note = pattern.lanes.find((lane) => lane.channel === ids.channel)!
    .notes[0].id
  const selectedNote = pattern.lanes.find(
    (lane) => lane.channel === ids.channel
  )!.notes[0]
  return {
    project: withPlugin,
    ids: {
      ...ids,
      pattern: pattern.id,
      note,
      selectedNote,
      pluginChannel,
      pluginEffect,
      arrangement,
      group,
      parentGroup,
      clipGroup,
      takeGroup,
      spareAudioClip,
      project: withPlugin,
      mixerTrack: withPlugin.mixer.tracks.find(
        (track) => track.id === ids.track
      )!,
      sourceClip: withPlugin.playlist.clips.find(
        (clip) => clip.id === ids.audioClip
      )!,
      audioTakeGroup: withPlugin.playlist.takeGroups!.find(
        (group) => group.id === takeGroup
      )!,
    },
  }
}

function pluginBinding() {
  return {
    target: { type: "instrument" as const, channel: 0 },
    format: "clap",
    path: "missing/test.clap",
    id: "test.synth",
    name: "Test plugin",
    state: [1],
    parameters: [
      {
        id: 7,
        name: "Level",
        min: 0,
        max: 1,
        value: 0.5,
        stepped: false,
        readOnly: false,
        automatable: true,
      },
    ],
  }
}

type Ids = ReturnType<typeof fullProject>["ids"]

/**
 * One small valid command of every type. The type makes the compiler ask
 * for a new entry when the Rust `Command` gains a variant.
 */
const EVERY_COMMAND: { [Type in Command["type"]]: (ids: Ids) => Command } = {
  insertReviewedNotes: (ids) => ({ type: "insertReviewedNotes", expected: ids.project.patterns.find((pattern) => pattern.id === ids.pattern)!, channel: ids.channel, notes: [{ start: 77, length: 120, key: 65 }], remove: [], extend: true }),
  setSampleRegions: (ids) => ({type:"setSampleRegions",sample:ids.sample,expected:[],regions:[{name:"Region",startFrame:0,endFrame:100}]}),
  setSamplePlayback: (ids) => ({ type: "setSamplePlayback", channel: ids.slicer, asset: { ...defaultAsset(), sample: ids.sample, sourceBpm: 90 } }),
  generateSlicePattern: (ids) => ({ type: "generateSlicePattern", channel: ids.slicer, pattern: ids.pattern }),
  setStageMedia: () => ({ type: "setStageMedia", media: { pads: [], quantization: "bar", videos: [], visual: { scene: "orbit", intensity: 1, hue: 180 }, danceChannel: null } }),
  addArrangement: (ids) => ({
    type: "addArrangement",
    name: "New",
    clips: [ids.audioClip],
    tracks: [ids.lowerTrack],
  }),
  renameArrangement: (ids) => ({
    type: "renameArrangement",
    id: ids.arrangement,
    name: "Verse",
  }),
  setArrangementReferences: (ids) => ({
    type: "setArrangementReferences",
    id: ids.arrangement,
    clips: [ids.audioClip],
    tracks: [ids.lowerTrack],
  }),
  switchArrangement: (ids) => ({
    type: "switchArrangement",
    id: ids.arrangement,
  }),
  removeArrangement: (ids) => ({
    type: "removeArrangement",
    id: ids.arrangement,
  }),
  addTrackGroup: () => ({ type: "addTrackGroup", name: "New", parent: null }),
  renameTrackGroup: (ids) => ({
    type: "renameTrackGroup",
    id: ids.group,
    name: "Percussion",
  }),
  moveTrackGroup: (ids) => ({
    type: "moveTrackGroup",
    id: ids.group,
    parent: ids.parentGroup,
  }),
  moveTrackToGroup: (ids) => ({
    type: "moveTrackToGroup",
    track: ids.playlistTrack,
    parent: ids.group,
  }),
  removeTrackGroup: (ids) => ({ type: "removeTrackGroup", id: ids.group }),
  addClipGroup: (ids) => ({
    type: "addClipGroup",
    clips: [
      ids.project.playlist.clips.find(
        (clip) => clip.content.type === "automation"
      )!.id,
      ids.spareAudioClip,
    ],
  }),
  removeClipGroup: (ids) => ({ type: "removeClipGroup", id: ids.clipGroup }),
  linkTrack: (ids) => ({
    type: "linkTrack",
    track: ids.playlistTrack,
    kind: { type: "instrument", channel: ids.channel },
  }),
  makeUnique: (ids) => ({ type: "makeUnique", clip: ids.clip }),
  addNotesWithCurves: (ids) => ({
    type: "addNotesWithCurves",
    pattern: ids.pattern,
    channel: ids.channel,
    notes: [{ start: 100, length: 50, key: 65, velocity: 0.5 }],
    curves: [],
  }),
  setNoteExpressionCurves: (ids) => ({
    type: "setNoteExpressionCurves",
    pattern: ids.pattern,
    channel: ids.channel,
    expected: [ids.selectedNote],
    expectedCurves: [],
    curves: [
      {
        note: ids.note,
        parameter: "finePitchCents",
        points: [{ position: 0, value: 0.5, curve: 0, hold: false }],
      },
    ],
  }),
  setPluginSidechainInput: (ids) => ({
    type: "setPluginSidechainInput",
    target: { type: "effect", track: ids.track, effect: ids.pluginEffect },
    input: 1,
  }),
  setSidechain: (ids) => ({
    type: "setSidechain",
    from: ids.track,
    to: 0,
    gain: 0.5,
  }),
  applyMixerTrackPreset: (ids) => ({
    type: "applyMixerTrackPreset",
    id: ids.track,
    expected: ids.mixerTrack,
    nameColor: true,
    preset: {
      version: 1,
      name: "Preset",
      color: 0x112233,
      volume: 0.5,
      pan: 0,
      muted: false,
      processing: ids.mixerTrack.processing ?? DEFAULT_TRACK_PROCESSING,
      latencyOffsetMs: 0,
      effects: [],
      plugins: [],
    },
  }),
  applyChannelPreset: (ids) => {
    const channel = ids.project.channels.find(
      (candidate) => candidate.id === ids.channel
    )!
    return {
      type: "applyChannelPreset",
      id: ids.channel,
      expected: channel,
      nameColor: false,
      preset: {
        name: "Preset",
        color: 0x112233,
        // A level the channel does not have, so the preset changes it.
        volume: channel.volume === 0.25 ? 0.5 : 0.25,
        pan: 0,
        // A preset never names a project sample: its sampler is empty
        // until audio is given with the command.
        source:
          channel.source.type === "sampler"
            ? { ...channel.source, sample: null }
            : channel.source,
      },
    }
  },
  applyPluginPreset: (ids) => {
    const bound = ids.project.plugins!.find(
      (plugin) =>
        plugin.target.type === "effect" &&
        plugin.target.effect === ids.pluginEffect
    )!
    // A preset holds a hosted plugin against a placeholder target.
    const binding = { ...bound, target: { type: "effect" as const, effect: 0 } }
    return {
      type: "applyPluginPreset",
      target: { type: "effect", effect: ids.pluginEffect },
      expected: { type: "hosted", instrument: false, binding },
      preset: {
        state: {
          type: "hosted",
          instrument: false,
          binding: { ...binding, state: [9, 9, 9] },
        },
      },
    }
  },
  moveMixerTracks: (ids) => ({
    type: "moveMixerTracks",
    expected: ids.project.mixer.tracks.map((track) => track.id),
    ids: [ids.track],
    before: ids.project.mixer.tracks[1].id,
  }),
  setTrackParam: (ids) => ({
    type: "setTrackParam",
    id: ids.track,
    param: 0,
    value: 0,
  }),
  ensureCurrentMixerTrack: () => ({ type: "ensureCurrentMixerTrack" }),
  setTrackExternalOutput: (ids) => ({
    type: "setTrackExternalOutput",
    id: ids.track,
    route: { left: 0, right: 1, exclusive: true },
  }),
  createAudioTakeGroup: (ids) => ({
    type: "createAudioTakeGroup",
    name: "New Takes",
    lanes: [{ name: "Mic", takes: [{ pass: 1, clip: ids.spareAudioClip }] }],
  }),
  renameAudioTakeGroup: (ids) => ({
    type: "renameAudioTakeGroup",
    id: ids.takeGroup,
    name: "Comp Takes",
  }),
  removeAudioTakeGroup: (ids) => ({
    type: "removeAudioTakeGroup",
    id: ids.takeGroup,
  }),
  auditionAudioTakeGroup: (ids) => ({
    type: "auditionAudioTakeGroup",
    id: ids.takeGroup,
    pass: 1,
  }),
  compAudioTakeGroup: (ids) => ({
    type: "compAudioTakeGroup",
    expected: ids.audioTakeGroup,
    sources: [ids.sourceClip],
    ranges: [{ pass: 1, start: 0, end: 960 }],
    name: "Comp",
    fadeTicks: 0,
    muteSources: true,
    replaceComp: false,
  }),
  compAudioClips: (ids) => ({
    type: "compAudioClips",
    sources: [ids.sourceClip],
    segments: [{ clip: ids.audioClip, start: 0, end: 960 }],
    destination: null,
    name: "Comp",
    fadeTicks: 0,
    muteSources: true,
  }),
  editPatternTimeline: (ids) => ({
    type: "editPatternTimeline",
    pattern: ids.pattern,
    expected: ids.project.patterns[0].timeline ?? { meters: [], markers: [] },
    expectedSignature: ids.project.patterns[0].timeSignature ?? null,
    edit: { type: "setSignature", signature: { numerator: 3, denominator: 4 } },
  }),
  replaceNotebook: () => ({
    type: "replaceNotebook",
    notebook: { pages: [{ title: "Notes", body: "QA" }] },
  }),
  setChannelVoiceSettings: (ids) => ({
    type: "setChannelVoiceSettings",
    id: ids.channel,
    settings: {
      ...createDefaultChannelVoiceSettings(),
      echo: { ...createDefaultChannelVoiceSettings().echo, enabled: true },
    },
  }),
  setChannelGroup: (ids) => ({
    type: "setChannelGroup",
    channels: [ids.channel],
    group: "New",
  }),
  renameChannelGroup: () => ({
    type: "renameChannelGroup",
    name: "Drums",
    newName: "Percussion",
  }),
  removeChannelGroup: () => ({ type: "removeChannelGroup", name: "Drums" }),
  fillStepRange: (ids) => ({
    type: "fillStepRange",
    pattern: ids.pattern,
    channel: ids.channel,
    lengthSteps: ids.project.patterns[0].lengthSteps,
    expected: ids.project.patterns[0].lanes.find(
      (lane) => lane.channel === ids.channel
    )!.notes,
    startStep: 0,
    endStep: 4,
    replace: true,
    notes: [{ start: 0, length: 120, key: 64, velocity: 0.5 }],
  }),
  updateCapturedNotes: (ids) => ({
    type: "updateCapturedNotes",
    pattern: ids.pattern,
    channel: ids.channel,
    expected: [ids.selectedNote],
    updates: [{ id: ids.note, patch: { velocity: 0.25 } }],
  }),
  generateAutomationLfo: (ids) => ({
    type: "generateAutomationLfo",
    expected: ids.project.automations.find(
      (automation) => automation.id === ids.automation
    )!,
    start: 0,
    end: 960,
    resolution: 120,
    lfo: {
      wave: "sine",
      period: 960,
      phase: 0,
      center: 0.5,
      depth: 0.25,
      width: 0.5,
      seed: 1,
    },
  }),
  addMeterChange: () => ({
    type: "addMeterChange",
    tick: 7400,
    signature: { numerator: 3, denominator: 4 },
  }),
  updateMeterChange: (ids) => ({
    type: "updateMeterChange",
    id: ids.meter,
    tick: 4003,
    signature: { numerator: 3, denominator: 4 },
  }),
  removeMeterChange: (ids) => ({ type: "removeMeterChange", id: ids.meter }),
  addTimelineMarker: () => ({
    type: "addTimelineMarker",
    tick: 5000,
    name: "Pause",
    kind: { type: "pause" },
  }),
  updateTimelineMarker: (ids) => ({
    type: "updateTimelineMarker",
    marker: {
      id: ids.marker,
      tick: 7401,
      name: "Chorus",
      kind: { type: "named" },
    },
  }),
  removeTimelineMarker: (ids) => ({
    type: "removeTimelineMarker",
    id: ids.marker,
  }),
  transformNotes: (ids) => ({
    type: "transformNotes",
    pattern: ids.pattern,
    channel: ids.channel,
    notes: [ids.selectedNote],
    transform: { type: "staccato", factor: 0.5 },
  }),
  addPluginInstrument: () => ({
    type: "addPluginInstrument",
    plugin: pluginBinding(),
  }),
  addPluginEffect: (ids) => ({
    type: "addPluginEffect",
    track: ids.track,
    plugin: pluginBinding(),
  }),
  setPluginParam: (ids) => ({
    type: "setPluginParam",
    target: { type: "instrument", channel: ids.pluginChannel },
    id: 7,
    value: 0.75,
  }),
  setPluginState: (ids) => ({
    type: "setPluginState",
    target: { type: "instrument", channel: ids.pluginChannel },
    state: [1, 2],
  }),
  setPluginWrapper: (ids) => ({
    type: "setPluginWrapper",
    target: { type: "instrument", channel: ids.pluginChannel },
    options: { smartDisable: true, fixedBuffer: 128, processing: "automatic", editorScale: 1.5, detached: true },
  }),
  updateSettings: () => ({ type: "updateSettings", patch: { swing: 0.25 } }),
  addSample: () => ({
    type: "addSample",
    name: "Rim",
    path: { kind: "factory", path: "Drums/Percussion/Rim 01.wav" },
  }),
  removeSample: (ids) => ({ type: "removeSample", id: ids.spareSample }),
  setSampleProcessing: (ids) => ({
    type: "setSampleProcessing", id: ids.spareSample,
    processing: {
      sourceClip: { id: ids.audioClip, track: ids.lowerTrack, start: 0, length: 1920,
        offset: 0, muted: false, content: { type: "audio", sample: ids.sample, mixerTrack: ids.track,
          output: "mixer", normalize: false, gain: 1, pan: 0, fadeIn: 0, fadeOut: 0,
          reverse: false, pitch: 0, stretch: { mode: "tape" } } },
      analysis: { frames: 16000, sampleRate: 16000, polyphonic: false, notes: [], transients: [] },
      overrides: [], markers: [{ sourceFrame: 0, targetFrame: 0 }, { sourceFrame: 16000, targetFrame: 16000 }],
      denoise: null,
    },
  }),
  addChannel: (ids) => ({ type: "addChannel", sample: ids.sample, index: 0 }),
  removeChannel: (ids) => ({ type: "removeChannel", id: ids.channel }),
  duplicateChannel: (ids) => ({ type: "duplicateChannel", id: ids.channel }),
  moveChannel: (ids) => ({ type: "moveChannel", id: ids.channel, index: 2 }),
  updateChannel: (ids) => ({
    type: "updateChannel",
    id: ids.channel,
    patch: { name: "Boom", color: 0x112233, mixerTrack: ids.track },
  }),
  setChannelSample: (ids) => ({ type: "setChannelSample", id: ids.channel }),
  updateSampler: (ids) => ({
    type: "updateSampler",
    id: ids.channel,
    patch: { rootKey: 48, start: 0.1, end: 0.9, reverse: true, cutGroup: 3 },
  }),
  setSamplerEnvelope: (ids) => ({
    type: "setSamplerEnvelope",
    id: ids.channel,
    envelope: { attackMs: 5, decayMs: 100, sustain: 0.5, releaseMs: 80 },
  }),
  setInstrumentParam: (ids) => ({
    type: "setInstrumentParam",
    channel: ids.synth,
    param: 1,
    value: 0.5,
  }),
  setInstrumentAsset: (ids) => ({
    type: "setInstrumentAsset", channel: ids.keyDrum,
    expected: ids.project.channels.find(channel => channel.id === ids.keyDrum)!.source,
    asset: { type: "drumKeys", patches: [{ key: 36, params: { mode: "snare", pitchHz: 170, decayMs: 400, tone: 0.6, snap: 0.8, level: 0.8 } }] },
  }),
  setSampleInstrumentAsset: (ids) => ({
    type: "setSampleInstrumentAsset", channel: ids.sampleInstrument,
    expected: null, asset: { type: "zones", zones: [] },
  }),
  editSampleZones: (ids) => ({
    type: "editSampleZones", channel: ids.sampleInstrument, expected: [], zones: [],
  }),
  setInstrumentParams: (ids) => ({
    type: "setInstrumentParams",
    channel: ids.synth,
    params: {
      ...descriptors.instruments.subtractiveSynth.defaults,
      glideMs: 40,
    } as InstrumentParams,
  }),
  addPattern: () => ({ type: "addPattern" }),
  removePattern: (ids) => ({ type: "removePattern", id: ids.sparePattern }),
  duplicatePattern: (ids) => ({ type: "duplicatePattern", id: ids.pattern }),
  movePattern: (ids) => ({ type: "movePattern", id: ids.pattern, index: 1 }),
  updatePattern: (ids) => ({
    type: "updatePattern",
    id: ids.pattern,
    patch: { lengthSteps: 32 },
  }),
  toggleStep: (ids) => ({
    type: "toggleStep",
    pattern: ids.pattern,
    channel: ids.channel,
    step: 3,
  }),
  addNotes: (ids) => ({
    type: "addNotes",
    pattern: ids.pattern,
    channel: ids.channel,
    notes: [{ start: 100, length: 50, key: 64, velocity: 0.5 }],
  }),
  removeNotes: (ids) => ({
    type: "removeNotes",
    pattern: ids.pattern,
    channel: ids.channel,
    notes: [ids.note],
  }),
  updateNotes: (ids) => ({
    type: "updateNotes",
    pattern: ids.pattern,
    channel: ids.channel,
    updates: [{ id: ids.note, patch: { key: 72, pan: -0.5 } }],
  }),
  clearLane: (ids) => ({
    type: "clearLane",
    pattern: ids.pattern,
    channel: ids.channel,
  }),
  addMixerTrack: () => ({ type: "addMixerTrack", name: "Bus" }),
  removeMixerTrack: (ids) => ({ type: "removeMixerTrack", id: ids.track }),
  updateMixerTrack: (ids) => ({
    type: "updateMixerTrack",
    id: ids.track,
    patch: { volume: 0.5, solo: true },
  }),
  setTrackOutput: (ids) => ({ type: "setTrackOutput", id: ids.track }),
  setSend: (ids) => ({ type: "setSend", from: ids.track, to: 0, gain: 0.5 }),
  addEffect: (ids) => ({ type: "addEffect", track: ids.track, kind: "reverb" }),
  removeEffect: (ids) => ({
    type: "removeEffect",
    track: ids.track,
    effect: ids.effect,
  }),
  moveEffect: (ids) => ({
    type: "moveEffect",
    track: ids.track,
    effect: ids.effect,
    index: 1,
  }),
  updateEffect: (ids) => ({
    type: "updateEffect",
    track: ids.track,
    effect: ids.effect,
    patch: { enabled: false, mix: 0.5 },
  }),
  setEffectParam: (ids) => ({
    type: "setEffectParam",
    track: ids.track,
    effect: ids.effect,
    param: 0,
    value: -30,
  }),
  setConvolverImpulse: (ids) => ({ type: "setConvolverImpulse", track: ids.track, effect: ids.convolver, impulse: { name: "Test IR", sampleRate: 48000, frames: [[1, .5], [0, .25]] } }),
  setEffectParams: (ids) => ({
    type: "setEffectParams",
    track: ids.track,
    effect: ids.effect,
    params: {
      ...descriptors.effects.compressor.defaults,
      ratio: 8,
    } as EffectParams,
  }),
  duplicateEffect: (ids) => ({
    type: "duplicateEffect",
    track: ids.track,
    effect: ids.effect,
  }),
  replaceEffect: (ids) => ({
    type: "replaceEffect",
    track: ids.track,
    effect: ids.effect,
    kind: "reverb",
  }),
  addPlaylistTrack: () => ({ type: "addPlaylistTrack", index: 0 }),
  movePlaylistTrack: (ids) => ({
    type: "movePlaylistTrack",
    id: ids.playlistTrack,
    index: 1,
  }),
  removePlaylistTrack: (ids) => ({
    type: "removePlaylistTrack",
    id: ids.playlistTrack,
  }),
  updatePlaylistTrack: (ids) => ({
    type: "updatePlaylistTrack",
    id: ids.playlistTrack,
    patch: { muted: true },
  }),
  addClips: (ids) => ({
    type: "addClips",
    clips: [
      {
        track: ids.playlistTrack,
        start: 3840,
        length: 960,
        content: { type: "pattern", pattern: ids.pattern },
      },
      {
        track: ids.playlistTrack,
        start: 7680,
        length: 480,
        offset: 120,
        muted: true,
        content: {
          type: "audio",
          sample: ids.sample,
          mixerTrack: 0,
          gain: 0.5,
          pan: -0.25,
          fadeIn: 60,
          fadeOut: 120,
          reverse: true,
          pitch: 7,
        },
      },
      {
        track: ids.playlistTrack,
        start: 9600,
        content: { type: "automation", automation: ids.automation },
      },
    ],
  }),
  removeClips: (ids) => ({ type: "removeClips", clips: [ids.clip] }),
  updateClips: (ids) => ({
    type: "updateClips",
    updates: [{ id: ids.clip, patch: { start: 960, offset: 240 } }],
  }),
  updateAudioClips: (ids) => ({
    type: "updateAudioClips",
    updates: [
      {
        id: ids.audioClip,
        patch: { gain: 0.5, pitch: -12, reverse: true, fadeIn: 240 },
      },
    ],
  }),
  addAutomation: (ids) => ({
    type: "addAutomation",
    target: { type: "trackPan", track: ids.track },
  }),
  removeAutomation: (ids) => ({
    type: "removeAutomation",
    id: ids.automation,
  }),
  updateAutomation: (ids) => ({
    type: "updateAutomation",
    id: ids.automation,
    patch: { name: "Ride", color: 0x334455 },
  }),
  setAutomationPoints: (ids) => ({
    type: "setAutomationPoints",
    id: ids.automation,
    points: [
      { tick: 0, value: 0, curve: 0.5, hold: false },
      { tick: 3840, value: 1, curve: 0, hold: true },
    ],
  }),
  duplicateAutomation: (ids) => ({
    type: "duplicateAutomation",
    id: ids.automation,
  }),
  addArrangementLayout: () => ({
    type: "addArrangementLayout",
    name: "Layout",
    copy: true,
  }),
  addInstrumentTrack: () => ({ type: "addInstrumentTrack" }),
  addAudioTrack: () => ({ type: "addAudioTrack" }),
  setPatternEvents: (ids) => ({
    type: "setPatternEvents",
    pattern: ids.pattern,
    target: { type: "trackVolume", track: ids.track },
    expected: [],
    points: [{ tick: 0, value: 0.5, curve: 0, hold: false }],
  }),
  generatePatternEventLfo: (ids) => ({
    type: "generatePatternEventLfo",
    pattern: ids.pattern,
    target: { type: "trackVolume", track: ids.track },
    expected: [],
    start: 0,
    end: 960,
    resolution: 120,
    lfo: {
      wave: "sine",
      period: 960,
      phase: 0,
      center: 0.5,
      depth: 0.25,
      width: 0.5,
      seed: 1,
    },
  }),
  generateNoteCurveLfo: (ids) => ({
    type: "generateNoteCurveLfo",
    pattern: ids.pattern,
    channel: ids.channel,
    expected: [ids.selectedNote],
    expectedCurves: [],
    parameter: "pan",
    origin: 0,
    resolution: 120,
    strength: 1,
    lfo: {
      wave: "sine",
      period: 960,
      phase: 0,
      center: 0.5,
      depth: 0.25,
      width: 0.5,
      seed: 1,
    },
  }),
  setEffectOutput: (ids) => ({
    type: "setEffectOutput",
    effect: ids.tap,
    output: 0,
    target: 0,
  }),
  setInternalControllerSettings: () => ({
    type: "setInternalControllerSettings",
    settings: [],
  }),
  setControllerLinks: (ids) => ({
    type: "setControllerLinks",
    links: [
      {
        source: { type: "midi", control: { type: "cc", number: 7 } },
        target: {
          type: "param",
          target: { type: "trackVolume", track: ids.track },
        },
        map: {
          min: 0,
          max: 1,
          smoothingMs: 0,
          pickup: "inherit",
          mode: "absolute",
        },
      },
    ],
  }),
  setChannelExpressionMap: (ids) => ({
    type: "setChannelExpressionMap",
    id: ids.channel,
    map: {
      modulationX: {
        destination: "pitch",
        plugin: "brightness",
        amount: 1,
        invert: false,
      },
      modulationY: {
        destination: "resonance",
        plugin: "expression",
        amount: 1,
        invert: false,
      },
      release: {
        destination: "release",
        plugin: "none",
        amount: 1,
        invert: false,
      },
      pitchRangeCents: 1200,
      bendRangeSemitones: 2,
    },
  }),
  batch: (ids) => ({
    type: "batch",
    label: "Two things",
    commands: [
      { type: "addPattern" },
      { type: "moveChannel", id: ids.channel, index: 1 },
    ],
  }),
  setChannelRack: (ids) => ({ type: "setChannelRack", id: ids.synth, graph: null }),
  setScriptLibrary: () => ({
    type: "setScriptLibrary",
    library: { assets: [{ reference: { id: "piano-fixture", revision: 1 }, languageVersion: 1, kind: "piano", name: "Fixture", source: "emit" }], channels: [], controllers: [] },
  }),
}

/** A project as the document reports it, apart from the id counter. */
const content = (project: Project) => ({ ...project, nextId: 0 })

describe("every command of the binding", () => {
  it("is covered: the binding and the list name the same commands", () => {
    const inBinding = [...commandBinding.matchAll(/"type": "(\w+)"/g)].map(
      (match) => match[1]
    )
    expect(inBinding.length).toBeGreaterThan(30)
    expect(Object.keys(EVERY_COMMAND).sort()).toEqual([...inBinding].sort())
  })

  it.each(Object.keys(EVERY_COMMAND) as Command["type"][])(
    "%s is applied by the WebAssembly document, undone and redone",
    (type) => {
      const { project, ids } = fullProject()
      const document = SimDocument.create(project)
      const before = document.snapshot(null).project

      const result = document.dispatch(EVERY_COMMAND[type](ids))
      const after = document.snapshot(null).project
      expect(content(after)).not.toEqual(content(before))
      expect(result.patch.history.entries).toHaveLength(1)
      // The copy kept from patches is the document's project exactly.
      expect(document.project()).toEqual(after)

      document.undo()
      expect(content(document.project())).toEqual(content(before))
      expect(document.isDirty()).toBe(false)
      document.redo()
      expect(document.project()).toEqual(after)
      expect(document.project()).toEqual(document.snapshot(null).project)
      document.dispose()
    }
  )
})

describe("SimDocument", () => {
  it("follows a history jump across added, moved and removed patterns", () => {
    const document = SimDocument.create(demoProject())
    const first = document.project().patterns[0].id
    const [second] = document.dispatch({ type: "addPattern" }).created
    document.dispatch({ type: "movePattern", id: second, index: 0 })
    document.dispatch({ type: "removePattern", id: first })
    document.dispatch({ type: "addChannel" })
    expect(document.project().patterns.map((pattern) => pattern.id)).toEqual([
      second,
    ])

    for (const cursor of [1, 4, 2, 0, 3]) {
      const patch = document.jump(cursor)
      expect(patch.history.cursor).toBe(cursor)
      expect(document.project()).toEqual(document.snapshot(null).project)
    }
    expect(document.isDirty()).toBe(true)
    document.dispose()
  })

  it("knows the id counter and the dirty flag without asking twice", () => {
    const document = SimDocument.create(emptyProject())
    expect(document.project().nextId).toBe(2)
    expect(document.isDirty()).toBe(false)

    document.dispatch({ type: "addChannel" })
    expect(document.project().nextId).toBe(4)
    expect(document.isDirty()).toBe(true)

    // Undo takes the channel away, but never hands its ids out again.
    document.undo()
    expect(document.project().nextId).toBe(4)
    expect(document.isDirty()).toBe(false)
    expect(document.dispatch({ type: "addPattern" }).created).toEqual([4])

    expect(document.markSaved()).toMatchObject({ dirty: false })
    expect(document.isDirty()).toBe(false)
    document.dispose()
  })

  it("opens the file it wrote, and nothing that is not a project", () => {
    const document = SimDocument.create(demoProject())
    document.dispatch({ type: "addPattern", name: "Fill" })
    const reopened = SimDocument.open(document.fileText())
    expect(reopened.project()).toEqual(document.project())
    expect(reopened.isDirty()).toBe(false)
    expect(() => SimDocument.open("[]")).toThrow(
      "this is not a Windfall project"
    )
    document.dispose()
    reopened.dispose()
  })

  it("is closed for good by dispose", () => {
    const document = SimDocument.create(emptyProject())
    document.dispose()
    document.dispose()
    expect(() => document.undo()).toThrow("This document was closed.")
    expect(() => document.snapshot(null)).toThrow("This document was closed.")
  })
})
