import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { Command } from "@/bindings"
import type { ContextItem, InlineAction } from "@/components/context-actions"
import { runAction } from "@/lib/actions"
import { dispatch, undo, useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { MASTER_TRACK } from "@/lib/units"
import { settle } from "@/test/harness"

import { linkMenu, trackLink, trackLinkLabel } from "./linked-tracks"
import { GridMetrics } from "./metrics"
import { history, project, startPlaylist, ui } from "./test-utils"
import { TrackHeaders } from "./track-headers"

/*
 * Instrument and audio tracks through the playlist's actions, its track
 * headers and the real document: one step makes the channel, the mixer
 * track and the linked lane, and the document keeps the lane's rule.
 */

let app: Awaited<ReturnType<typeof startPlaylist>>

beforeEach(async () => {
  app = await startPlaylist()
})

afterEach(() => {
  app.stop()
  vi.restoreAllMocks()
})

async function created(command: Command): Promise<number[]> {
  const result = await dispatch(command)
  if (!result) throw new Error("The command was refused")
  return result.created
}

const lane = (id: number) => {
  const found = project().playlist.tracks.find((item) => item.id === id)
  if (!found) throw new Error(`No playlist track ${id}`)
  return found
}
const mixerOf = (clip: number) => {
  const found = project().playlist.clips.find((item) => item.id === clip)
  if (found?.content.type !== "audio") throw new Error("Not an audio clip")
  return found.content.mixerTrack
}
const audio = (sample: number, mixerTrack: number) => ({
  type: "audio" as const,
  sample,
  mixerTrack,
  gain: 1,
  pan: 0,
  fadeIn: 0,
  fadeOut: 0,
  reverse: false,
  pitch: 0,
})

/** The entries of a "Link track" submenu that can be run. */
function entries(item: ContextItem): InlineAction[] {
  if (typeof item === "string" || !("submenu" in item)) return []
  return item.items.filter(
    (entry): entry is InlineAction =>
      typeof entry !== "string" && "title" in entry
  )
}
const entry = (track: number, title: string) => {
  const found = entries(linkMenu(track)).find((item) => item.title === title)
  if (!found) throw new Error(`No "${title}" in the link menu`)
  return found
}

describe("instrument and audio tracks", () => {
  it("New instrument track makes a channel, its mixer track and a linked lane in one step", async () => {
    const before = history().cursor
    const counts = {
      channels: project().channels.length,
      mixer: project().mixer.tracks.length,
      lanes: project().playlist.tracks.length,
    }
    const send = vi.spyOn(app.backend, "dispatch")

    await runAction("playlist.addInstrumentTrack")

    expect(send).toHaveBeenCalledExactlyOnceWith(
      { type: "addInstrumentTrack" },
      undefined
    )
    expect(history().cursor).toBe(before + 1)
    expect(history().entries.at(-1)?.label).toBe("Add instrument track")
    const channel = project().channels.at(-1)!
    const made = project().playlist.tracks.at(-1)!
    expect(project().channels).toHaveLength(counts.channels + 1)
    expect(project().mixer.tracks).toHaveLength(counts.mixer + 1)
    expect(project().playlist.tracks).toHaveLength(counts.lanes + 1)
    expect(project().mixer.tracks.at(-1)).toMatchObject({
      id: channel.mixerTrack,
      name: channel.name,
    })
    expect(made.name).toBe(channel.name)
    expect(trackLink(project(), made.id)).toEqual({
      type: "instrument",
      channel: channel.id,
    })
    expect(trackLinkLabel(project(), made.id)).toBe(channel.name)
    // The new track is the target, with its channel and mixer track in view.
    expect(ui().targetTrack).toBe(made.id)
    expect(useUiStore.getState().selectedChannel).toBe(channel.id)
    expect(useUiStore.getState().selectedTrack).toBe(channel.mixerTrack)

    await undo()
    expect(project().channels).toHaveLength(counts.channels)
    expect(project().mixer.tracks).toHaveLength(counts.mixer)
    expect(project().playlist.tracks).toHaveLength(counts.lanes)
  })

  it("New audio track routes every audio clip placed on it to its mixer track", async () => {
    await runAction("playlist.addAudioTrack")
    const made = project().playlist.tracks.at(-1)!
    const mixer = project().mixer.tracks.at(-1)!
    expect(history().entries.at(-1)?.label).toBe("Add audio track")
    expect(made.name).toBe(mixer.name)
    expect(trackLink(project(), made.id)).toEqual({
      type: "audioTrack",
      mixer: mixer.id,
    })
    expect(trackLinkLabel(project(), made.id)).toBe(mixer.name)
    expect(useUiStore.getState().selectedTrack).toBe(mixer.id)

    const sample = project().samples[0].id
    const [plain] = await created({ type: "addPlaylistTrack", name: "Plain" })
    const [onAudio, onPlain] = await created({
      type: "addClips",
      clips: [
        { track: made.id, start: 0, length: 960, content: audio(sample, MASTER_TRACK) },
        { track: plain, start: 960, length: 960, content: audio(sample, MASTER_TRACK) },
      ],
    })
    expect(mixerOf(onAudio)).toBe(mixer.id)
    expect(mixerOf(onPlain)).toBe(MASTER_TRACK)

    // Dragged onto the audio track, a clip is rerouted in the same undo step.
    const before = history().cursor
    await dispatch({
      type: "updateClips",
      updates: [{ id: onPlain, patch: { track: made.id } }],
    })
    expect(mixerOf(onPlain)).toBe(mixer.id)
    expect(history().cursor).toBe(before + 1)
    await undo()
    expect(mixerOf(onPlain)).toBe(MASTER_TRACK)

    // Deleting the mixer track drops the link and keeps the lane.
    await dispatch({ type: "removeMixerTrack", id: mixer.id })
    expect(trackLink(project(), made.id)).toBeNull()
    expect(lane(made.id).name).toBe(made.name)
    expect(mixerOf(onAudio)).toBe(MASTER_TRACK)
  })

  it("links, relinks and unlinks a lane from its header menu, and shows the link", async () => {
    const [plain] = await created({ type: "addPlaylistTrack", name: "Plain" })
    const channel = project().channels[0]
    const bus = project().mixer.tracks.find((item) => item.id !== MASTER_TRACK)!
    render(<TrackHeaders metrics={new GridMetrics()} />)
    const header = () => screen.getByRole("group", { name: "Plain" })
    expect(trackLink(project(), plain)).toBeNull()
    expect(entry(plain, "Not linked").checked).toBe(true)

    await act(async () => {
      await entry(plain, channel.name).run()
      await settle()
    })
    expect(trackLink(project(), plain)).toEqual({
      type: "instrument",
      channel: channel.id,
    })
    expect(header()).toHaveTextContent(`Plain · ${channel.name}`)
    expect(entry(plain, channel.name).checked).toBe(true)
    expect(history().entries.at(-1)?.label).toBe("Link playlist track")

    // Pressing the header brings the channel and its mixer track into view.
    useUiStore.setState({ selectedChannel: null, selectedTrack: null })
    const send = vi.spyOn(app.backend, "dispatch")
    fireEvent.pointerDown(header())
    expect(useUiStore.getState().selectedChannel).toBe(channel.id)
    expect(useUiStore.getState().selectedTrack).toBe(channel.mixerTrack)
    expect(send).not.toHaveBeenCalled()

    // The mixer tracks are listed under the same names as the channels of
    // the demo kit, so the audio-track entry is found by its place.
    const mixerEntry = entries(linkMenu(plain)).filter(
      (item) => item.title === bus.name
    ).at(-1)!
    await act(async () => {
      await mixerEntry.run()
      await settle()
    })
    expect(trackLink(project(), plain)).toEqual({
      type: "audioTrack",
      mixer: bus.id,
    })
    expect(header()).toHaveTextContent(`Plain · ${bus.name}`)
    useUiStore.setState({ selectedChannel: null, selectedTrack: null })
    fireEvent.pointerDown(header())
    expect(useUiStore.getState().selectedTrack).toBe(bus.id)
    expect(useUiStore.getState().selectedChannel).toBeNull()

    await act(async () => {
      await entry(plain, "Not linked").run()
      await settle()
    })
    expect(trackLink(project(), plain)).toBeNull()
    expect(header()).not.toHaveTextContent("·")
  })

  it("keeps a link through save and reopen", async () => {
    await runAction("playlist.addAudioTrack")
    await runAction("playlist.addInstrumentTrack")
    const links = structuredClone(
      useProjectStore.getState().project.playlist.arrangementBook?.linkedTracks
    )
    expect(Object.keys(links ?? {})).toHaveLength(2)
    const path = await app.backend.projectSave("/projects/linked.windfall")
    await app.backend.projectNew()
    await settle()
    await app.backend.projectOpen(path)
    await settle()
    expect(project().playlist.arrangementBook?.linkedTracks).toEqual(links)
  })
})
