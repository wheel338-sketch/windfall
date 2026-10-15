import { act, render, screen } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"

import { disabledReason, getAppState, registry, runAction } from "@/lib/actions"
import { useProjectStore } from "@/lib/store/project"
import { useAudioEditorRequest } from "@/features/audio-editor/request"
import { usePlaylistStore } from "../store"
import { startPlaylist, ui } from "../test-utils"
import { ClipInspector } from "./clip-inspector"

let stop: (() => void) | undefined
afterEach(() => {
  stop?.()
  vi.restoreAllMocks()
  useAudioEditorRequest.setState({ pending: false })
})

it("opens the audio editor for the selected clip from the command palette, even with the settings hidden", async () => {
  const app = await startPlaylist()
  stop = app.stop
  const action = registry.get("playlist.audioEditor")!
  expect(disabledReason(action, getAppState())).toBe("Select one audio clip")

  const added = await app.backend.addAudioClipFromFile("/factory/Loops/Drum loop 128.wav", { start: 0 })
  const clip = useProjectStore.getState().project.playlist.clips.find((clip) => clip.id === added.created.at(-1))!
  vi.spyOn(app.backend, "audioEditorOpen").mockImplementation(async () => ({
    token: 1,
    clip: clip.id,
    name: "Take",
    frames: 1000,
    sampleRate: 1000,
    peaks: [-0.5, 0.5],
  }))
  ui().select([clip.id])
  act(() => usePlaylistStore.getState().toggleInspector())
  expect(usePlaylistStore.getState().inspectorOpen).toBe(false)
  render(<ClipInspector />)

  await act(() => runAction("playlist.audioEditor"))
  expect(usePlaylistStore.getState().inspectorOpen).toBe(true)
  expect(await screen.findByRole("img", { name: /Clip waveform/ })).toBeInTheDocument()
  expect(useAudioEditorRequest.getState().pending).toBe(false)
})
