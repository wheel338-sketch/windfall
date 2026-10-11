import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { runAction } from "@/lib/actions"
import type { Backend } from "@/lib/ipc"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { settle } from "@/test/harness"

import { ChannelInspector } from "./inspector"
import { withLayerChild } from "./layer"
import { channel, history, labels, project, startRack } from "./test-utils"
import {
  createDefaultChannelVoiceSettings,
  layerOf,
  MAX_LAYER_CHILDREN,
} from "./voice"

/*
 * Acceptance of the rack voice tools on the real project document: the
 * channel filter and its modulation, layer channels, and the honest refusal
 * of sample pre-processing in the browser. Audio is the native engine's and
 * is covered by its own tests.
 */

let backend: Backend
let stop: () => void
beforeEach(async () => {
  ;({ backend, stop } = await startRack())
  useUiStore.getState().selectChannel(channel("Kick").id)
})
afterEach(() => {
  stop()
  vi.restoreAllMocks()
})

async function open() {
  render(<ChannelInspector />)
  await settle()
}

const voice = (name: string) => channel(name).voice

it("saves filter, envelope and LFO edits as undo steps the panel follows", async () => {
  await open()
  const cursor = history().cursor
  await userEvent.click(
    screen.getByRole("switch", { name: "Enable channel filter" })
  )
  await waitFor(() =>
    expect(voice("Kick")?.envelopes.filterSettings?.enabled).toBe(true)
  )
  await userEvent.click(screen.getByRole("button", { name: "High-pass" }))
  await waitFor(() =>
    expect(voice("Kick")?.envelopes.filterSettings?.mode).toBe("highPass")
  )
  await userEvent.click(
    screen.getByRole("switch", { name: "Enable resonance envelope" })
  )
  await userEvent.click(
    screen.getByRole("switch", { name: "Enable volume LFO" })
  )
  await waitFor(() =>
    expect(voice("Kick")?.envelopes.lfos?.volume.enabled).toBe(true)
  )
  expect(voice("Kick")?.envelopes.resonance?.enabled).toBe(true)
  expect(history().cursor).toBe(cursor + 4)
  expect(labels().at(-1)).toBe("Set channel voice settings")

  // Undo shows through in the mounted panel without a remount.
  await act(async () => {
    await undo()
  })
  await waitFor(() =>
    expect(
      screen.getByRole("switch", { name: "Enable volume LFO" })
    ).not.toBeChecked()
  )
  expect(
    screen.getByRole("switch", { name: "Enable resonance envelope" })
  ).toBeChecked()
  await act(async () => {
    await redo()
  })
  await waitFor(() =>
    expect(
      screen.getByRole("switch", { name: "Enable volume LFO" })
    ).toBeChecked()
  )

  // Settings left at their defaults stay out of the saved channel.
  expect(voice("Snare")?.envelopes).not.toHaveProperty("filterSettings")
})

it("adds a layer channel, picks its children and keeps them consistent", async () => {
  const cursor = history().cursor
  const tracks = project().mixer.tracks.length
  await act(async () => {
    await runAction("channel.addLayer")
  })
  const layer = channel("Layer")
  expect(layerOf(layer.voice)).toEqual({ enabled: true, children: [] })
  expect(history().cursor).toBe(cursor + 1)
  expect(labels().at(-1)).toBe("Add layer channel")
  expect(project().mixer.tracks.length).toBe(tracks)
  expect(useUiStore.getState().selectedChannel).toBe(layer.id)

  await open()
  const kick = channel("Kick").id
  const snare = channel("Snare").id
  await userEvent.click(screen.getByRole("checkbox", { name: "Play on Kick" }))
  await waitFor(() =>
    expect(layerOf(voice("Layer")).children).toEqual([kick])
  )
  await userEvent.click(
    screen.getByRole("checkbox", { name: "Play on Snare" })
  )
  await waitFor(() =>
    expect(layerOf(voice("Layer")).children).toEqual([kick, snare])
  )
  expect(screen.queryByRole("checkbox", { name: "Play on Layer" })).toBeNull()
  expect(history().cursor).toBe(cursor + 3)

  // A loop of layers is refused and nothing changes.
  const before = project()
  await act(async () => {
    await dispatch({
      type: "setChannelVoiceSettings",
      id: kick,
      settings: {
        ...(channel("Kick").voice ?? createDefaultChannelVoiceSettings()),
        layer: { enabled: true, children: [layer.id] },
      },
    })
  })
  expect(project()).toBe(before)
  expect(history().cursor).toBe(cursor + 3)

  // Deleting a child takes it out of the layer in the same undo step.
  await act(async () => {
    await dispatch({ type: "removeChannel", id: kick })
  })
  expect(layerOf(voice("Layer")).children).toEqual([snare])
  await act(async () => {
    await undo()
  })
  expect(layerOf(voice("Layer")).children).toEqual([kick, snare])
  expect(channel("Kick").id).toBe(kick)

  // Unchecking removes a child; switching the layer off keeps the list.
  await userEvent.click(screen.getByRole("checkbox", { name: "Play on Kick" }))
  await waitFor(() =>
    expect(layerOf(voice("Layer")).children).toEqual([snare])
  )
  await userEvent.click(screen.getByRole("switch", { name: "Layer channel" }))
  await waitFor(() => expect(layerOf(voice("Layer")).enabled).toBe(false))
  expect(layerOf(voice("Layer")).children).toEqual([snare])
  expect(screen.queryByRole("checkbox", { name: "Play on Snare" })).toBeNull()
})

it("keeps a layer's children distinct, ordered and bounded", () => {
  expect(withLayerChild([3, 1], 2, true)).toEqual([3, 1, 2])
  expect(withLayerChild([3, 1], 3, true)).toEqual([1, 3])
  expect(withLayerChild([3, 1], 3, false)).toEqual([1])
  const full = Array.from({ length: MAX_LAYER_CHILDREN }, (_, index) => index)
  expect(withLayerChild(full, 99, true)).toHaveLength(MAX_LAYER_CHILDREN)
  expect(withLayerChild(full, 99, true)).not.toContain(99)
})

it("refuses sample pre-processing in the browser without changing anything", async () => {
  await open()
  const before = useProjectStore.getState().project
  const cursor = history().cursor
  expect(
    screen.getByText(/Browser mode cannot render a new sample/)
  ).toBeVisible()
  const apply = screen.getByRole("button", { name: "Apply to sample" })

  // Nothing chosen is refused before the backend is asked.
  const request = vi.spyOn(backend, "samplerPreprocess")
  await userEvent.click(apply)
  expect(screen.getByRole("alert")).toHaveTextContent(
    "Choose at least one treatment to apply."
  )
  expect(request).not.toHaveBeenCalled()

  await userEvent.click(
    screen.getByRole("checkbox", { name: "Normalize to −1 dBFS" })
  )
  await userEvent.click(
    screen.getByRole("checkbox", { name: "Reverse the audio" })
  )
  await userEvent.click(apply)
  expect(await screen.findByRole("alert")).toHaveTextContent(
    /desktop audio engine/
  )
  expect(request).toHaveBeenCalledWith({
    channel: channel("Kick").id,
    reverse: true,
    normalize: true,
  })
  expect(useProjectStore.getState().project).toBe(before)
  expect(history().cursor).toBe(cursor)
  // The loop seam needs a forward loop.
  expect(
    screen.getByText("Turn on a forward loop to crossfade its seam.")
  ).toBeVisible()
})
