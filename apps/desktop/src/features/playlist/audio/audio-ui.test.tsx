import { act, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { ClipContent } from "@/bindings"
import type { Backend } from "@/lib/ipc"
import { dispatch, undo, useProjectStore } from "@/lib/store/project"
import { setPlayMode, setTransportPattern } from "@/lib/store/transport"
import { settle } from "@/test/harness"

import PlaylistPanel from "../index"
import { BAR, history, labels, project, startPlaylist, ui } from "../test-utils"
import { addAudioFile } from "./ops"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

// jsdom has no canvas to draw on. The grid's pointer handling is tested on
// a stand-in surface; here it is everything around it.
vi.mock("@/lib/canvas/react", () => ({
  TimeGridCanvas: () => null,
}))

const LOOP = "/factory/Loops/Drum loop 128.wav"

type Audio = Extract<ClipContent, { type: "audio" }>

let backend: Backend
let stop: () => void

beforeEach(async () => {
  ;({ backend, stop } = await startPlaylist())
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null)
})
afterEach(() => stop())

async function flush() {
  await act(async () => {
    await settle()
  })
}

async function addLoop(start = 0): Promise<number> {
  let id: number | null = null
  await act(async () => {
    id = await addAudioFile(LOOP, { start })
    await settle()
  })
  return id as unknown as number
}

const audio = (id: number) =>
  project().playlist.clips.find((clip) => clip.id === id)?.content as Audio
const inspector = () =>
  screen.queryByRole("group", { name: "Audio clip settings" })
const steps = () => history().entries.length

describe("the audio clip settings", () => {
  it("keep their place while the song has an audio clip, selected or not", async () => {
    render(<PlaylistPanel />)
    expect(inspector()).toBeNull()
    const id = await addLoop()
    // A clip that was just added is the selection.
    expect(inspector()).not.toBeNull()
    expect(within(inspector()!).getByTitle("Drum loop 128")).toBeInTheDocument()

    // Selecting a clip must not move the timeline under the pointer, so the
    // strip stays, with its controls laid out unseen and out of reach.
    await act(async () => ui().clearSelection())
    const idle = inspector()!
    expect(idle).toHaveAttribute("data-idle")
    expect(idle).toHaveTextContent("Select an audio clip to edit it")
    expect(within(idle).queryByRole("slider")).toBeNull()
    const held = idle.querySelector("[inert]")!
    expect(held).toHaveAttribute("aria-hidden", "true")
    expect(
      within(held as HTMLElement).getAllByRole("slider", { hidden: true })
    ).toHaveLength(5)

    await act(async () => ui().select([id]))
    expect(inspector()).not.toHaveAttribute("data-idle")
    expect(
      within(inspector()!).getByRole("slider", { name: "Clip gain" })
    ).toBeInTheDocument()
  })

  it("show nothing to edit for a pattern clip, and are gone with the last audio clip", async () => {
    render(<PlaylistPanel />)
    await dispatch({ type: "addPlaylistTrack" })
    const added = await dispatch({
      type: "addClips",
      clips: [
        {
          track: project().playlist.tracks[0].id,
          start: 0,
          content: { type: "pattern", pattern: project().patterns[0].id },
        },
      ],
    })
    await act(async () => ui().select(added!.created))
    // No audio clip in the song: no strip.
    expect(inspector()).toBeNull()

    await addLoop()
    await act(async () => ui().select(added!.created))
    expect(inspector()).toHaveAttribute("data-idle")
  })

  it("set gain, pan and pitch from their controls, each as one undo step", async () => {
    render(<PlaylistPanel />)
    const id = await addLoop()
    const strip = within(inspector()!)

    const gain = strip.getByRole("slider", { name: "Clip gain" })
    expect(gain).toHaveAttribute("aria-valuetext", "0.0 dB")
    const before = steps()
    fireEvent.keyDown(gain, { key: "Home" })
    fireEvent.keyUp(gain, { key: "Home" })
    await flush()
    expect(audio(id).gain).toBe(0)
    expect(steps()).toBe(before + 1)
    expect(labels().at(-1)).toBe("Change clip gain")
    // Back to 0 dB with a double-click.
    fireEvent.doubleClick(gain)
    await flush()
    expect(audio(id).gain).toBe(1)

    const pan = strip.getByRole("slider", { name: "Clip pan" })
    fireEvent.keyDown(pan, { key: "End" })
    fireEvent.keyUp(pan, { key: "End" })
    await flush()
    expect(audio(id).pan).toBe(1)
    expect(labels().at(-1)).toBe("Change clip pan")

    const pitch = strip.getByRole("slider", {
      name: "Clip pitch in semitones",
    })
    fireEvent.keyDown(pitch, { key: "ArrowUp" })
    fireEvent.keyUp(pitch, { key: "ArrowUp" })
    await flush()
    expect(audio(id).pitch).toBe(1)
    expect(labels().at(-1)).toBe("Change clip pitch")
    // It says what the pitch does to the speed.
    expect(
      inspector()!.querySelector("[data-slot=clip-speed]")
    ).toHaveTextContent("1.06× speed")
  })

  it.each([undefined, false, true])(
    "dispatches one normalize patch for all selected clips when stored as %s",
    async (normalize) => {
      render(<PlaylistPanel />)
      const first = await addLoop(0)
      const second = await addLoop(4 * BAR)
      await act(async () => {
        useProjectStore.setState((state) => ({
          project: {
            ...state.project,
            playlist: {
              ...state.project.playlist,
              clips: state.project.playlist.clips.map((clip) =>
                clip.content.type === "audio"
                  ? { ...clip, content: { ...clip.content, normalize } }
                  : clip
              ),
            },
          },
        }))
        ui().select([first, second])
      })
      const toggle = within(inspector()!).getByRole("button", {
        name: "Normalize clip playback",
      })
      expect(toggle).toHaveAttribute("aria-pressed", String(normalize === true))
      const send = vi.spyOn(backend, "dispatch")
      fireEvent.click(toggle)
      await flush()
      expect(send).toHaveBeenCalledTimes(1)
      expect(send.mock.lastCall?.[0]).toEqual({
        type: "updateAudioClips",
        updates: [
          { id: first, patch: { normalize: normalize !== true } },
          { id: second, patch: { normalize: normalize !== true } },
        ],
      })
    }
  )

  it("reverse a clip and set its fades", async () => {
    render(<PlaylistPanel />)
    const id = await addLoop()
    const strip = within(inspector()!)

    fireEvent.click(strip.getByRole("button", { name: "Reverse" }))
    await flush()
    expect(audio(id).reverse).toBe(true)
    expect(strip.getByRole("button", { name: "Reverse" })).toHaveAttribute(
      "aria-pressed",
      "true"
    )

    const fadeIn = strip.getByRole("slider", { name: "Fade in" })
    expect(fadeIn).toHaveAttribute("aria-valuetext", "No fade")
    fireEvent.keyDown(fadeIn, { key: "End" })
    fireEvent.keyUp(fadeIn, { key: "End" })
    await flush()
    // A fade is at most as long as the clip.
    expect(audio(id).fadeIn).toBe(2 * BAR)
    expect(labels().at(-1)).toBe("Change clip fade")

    const fadeOut = strip.getByRole("slider", { name: "Fade out" })
    fireEvent.keyDown(fadeOut, { key: "Enter" })
    const entry = screen.getByRole("textbox", { name: "Fade out" })
    fireEvent.change(entry, { target: { value: "250 ms" } })
    fireEvent.keyDown(entry, { key: "Enter" })
    await flush()
    // A quarter of a second at 128 bpm.
    expect(audio(id).fadeOut).toBe(512)
  })

  it("change every selected audio clip together, in one undo step", async () => {
    render(<PlaylistPanel />)
    const first = await addLoop(0)
    const second = await addLoop(4 * BAR)
    await act(async () => ui().select([first, second]))
    const strip = within(inspector()!)
    expect(strip.getByText("2 audio clips")).toBeInTheDocument()

    const send = vi.spyOn(backend, "dispatch")
    const before = steps()
    const gain = strip.getByRole("slider", { name: "Clip gain" })
    fireEvent.keyDown(gain, { key: "Home" })
    fireEvent.keyUp(gain, { key: "Home" })
    await flush()

    expect(send.mock.lastCall?.[0]).toMatchObject({
      type: "updateAudioClips",
      updates: [
        { id: first, patch: { gain: 0 } },
        { id: second, patch: { gain: 0 } },
      ],
    })
    expect([audio(first).gain, audio(second).gain]).toEqual([0, 0])
    expect(steps()).toBe(before + 1)
    await act(async () => {
      await undo()
    })
    expect([audio(first).gain, audio(second).gain]).toEqual([1, 1])
  })

  it("route the clip to another mixer track, or to a new one", async () => {
    render(<PlaylistPanel />)
    const id = await addLoop()
    const route = () =>
      within(inspector()!).getByRole("button", { name: /^Playback route:/ })
    expect(route()).toHaveAccessibleName("Playback route: Drum loop 128")

    fireEvent.click(route())
    fireEvent.click(await screen.findByRole("menuitem", { name: "Kick" }))
    await flush()
    const kick = project().mixer.tracks.find((track) => track.name === "Kick")!
    expect(audio(id).mixerTrack).toBe(kick.id)
    expect(route()).toHaveAccessibleName("Playback route: Kick")

    const count = project().mixer.tracks.length
    fireEvent.click(route())
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "New mixer track" })
    )
    await flush()
    expect(project().mixer.tracks).toHaveLength(count + 1)
    expect(audio(id).mixerTrack).toBe(project().mixer.tracks.at(-1)?.id)
  })

  it("keep Delete and Ctrl+D on a setting from the clip", async () => {
    render(<PlaylistPanel />)
    const id = await addLoop()
    const gain = within(inspector()!).getByRole("slider", { name: "Clip gain" })
    fireEvent.keyDown(gain, { key: "Home" })
    fireEvent.keyUp(gain, { key: "Home" })
    await flush()
    const before = steps()
    act(() => gain.focus())
    // Used up: the clip is neither deleted nor copied, and the gain is not
    // put back either.
    expect(fireEvent.keyDown(gain, { key: "Delete", code: "Delete" })).toBe(
      false
    )
    expect(
      fireEvent.keyDown(gain, { key: "d", code: "KeyD", ctrlKey: true })
    ).toBe(false)
    await flush()
    expect(project().playlist.clips.map((clip) => clip.id)).toEqual([id])
    expect(audio(id).gain).toBe(0)
    expect(steps()).toBe(before)
  })

  it("keep presets, steps and scales in a menu beside each setting", async () => {
    render(<PlaylistPanel />)
    const id = await addLoop()
    const strip = within(inspector()!)
    // One button per setting, not rows of them.
    expect(strip.queryByRole("button", { name: "Halve" })).toBeNull()

    fireEvent.click(strip.getByRole("button", { name: "Gain presets" }))
    fireEvent.click(await screen.findByRole("menuitem", { name: "Halve" }))
    await flush()
    expect(audio(id).gain).toBe(0.5)

    fireEvent.click(strip.getByRole("button", { name: "Pan presets" }))
    // Nothing to halve at the center: the choice is there, but disabled.
    expect(
      await screen.findByRole("menuitem", { name: "Halve" })
    ).toHaveAttribute("aria-disabled", "true")
  })

  it("step aside when the clips they show are undone, until a clip is selected", async () => {
    render(<PlaylistPanel />)
    const first = await addLoop(0)
    await addLoop(4 * BAR)
    expect(inspector()).not.toHaveAttribute("data-idle")
    await act(async () => {
      await undo()
      await settle()
    })
    // The first loop is still in the song, but nothing is selected and the
    // clip the strip showed is gone: no empty strip is left behind.
    expect(project().playlist.clips.map((clip) => clip.id)).toEqual([first])
    expect(inspector()).toBeNull()

    await act(async () => ui().select([first]))
    expect(inspector()).not.toBeNull()
    expect(inspector()).not.toHaveAttribute("data-idle")
  })

  it("can be put away and brought back", async () => {
    render(<PlaylistPanel />)
    await addLoop()
    fireEvent.click(
      screen.getByRole("button", { name: "Hide the clip settings" })
    )
    expect(inspector()).toBeNull()
    expect(ui().inspectorOpen).toBe(false)
    await act(async () => ui().toggleInspector())
    expect(inspector()).not.toBeNull()
  })
})

describe("the sounds to place", () => {
  it("lists the project's samples and picks one as the brush", async () => {
    render(<PlaylistPanel />)
    const list = screen.getByRole("group", { name: "Sound to place" })
    const rows = within(list).getAllByRole("button")
    expect(rows.map((row) => row.textContent)).toEqual([
      "Kick",
      "Clap",
      "Hat",
      "Snare",
    ])
    const pattern = screen.getByRole("button", { name: /^Pattern 1/ })
    expect(pattern).toHaveAttribute("aria-pressed", "true")

    fireEvent.click(rows[1])
    const clap = project().samples[1]
    expect(ui().brush).toEqual({ type: "audio", sample: clap.id })
    expect(rows[1]).toHaveAttribute("aria-pressed", "true")
    // One thing is the brush at a time.
    expect(pattern).toHaveAttribute("aria-pressed", "false")

    fireEvent.click(pattern)
    await flush()
    expect(ui().brush).toEqual({ type: "pattern" })
    expect(rows[1]).toHaveAttribute("aria-pressed", "false")
  })

  it("counts the clips of a sample", async () => {
    render(<PlaylistPanel />)
    await addLoop(0)
    await addLoop(4 * BAR)
    const list = screen.getByRole("group", { name: "Sound to place" })
    expect(
      within(list).getByRole("button", { name: /^Drum loop 128/ })
    ).toHaveTextContent("Drum loop 128×2")
  })

  it("goes back to the pattern when one is selected elsewhere", async () => {
    render(<PlaylistPanel />)
    const added = await dispatch({ type: "addPattern", name: "Bass" })
    await act(async () =>
      ui().setBrush({ type: "audio", sample: project().samples[0].id })
    )
    await act(async () => {
      await setTransportPattern(added!.created[0])
    })
    expect(ui().brush).toEqual({ type: "pattern" })
  })
})

describe("the warning about clips that will be silent", () => {
  const warning = () =>
    document.querySelector<HTMLElement>("[data-slot=playlist-overlap-warning]")

  it("shows while more than 64 audio clips overlap, says where, and goes when they no longer do", async () => {
    render(<PlaylistPanel />)
    const first = await addLoop(4 * BAR)
    expect(warning()).toBeNull()

    // 64 more on the same spot, two bars further on: 65 from bar 7.
    const template = project().playlist.clips.find((clip) => clip.id === first)!
    const added = await dispatch({
      type: "addClips",
      clips: Array.from({ length: 64 }, () => ({
        track: template.track,
        start: 6 * BAR,
        length: 4 * BAR,
        content: template.content,
      })),
    })
    await dispatch({
      type: "updateClips",
      updates: [{ id: first, patch: { length: 4 * BAR } }],
    })
    await flush()
    expect(warning()).toHaveAccessibleName(
      "More than 64 audio clips overlap at bar 7; the extra ones will be silent. Go there"
    )
    expect(warning()).toHaveTextContent("Over 64 clips at bar 7")

    // Clicking it puts the song position there, in song mode as well,
    // where the playhead is the engine's.
    fireEvent.click(warning()!)
    expect(ui().cursorTick).toBe(6 * BAR)
    await act(async () => {
      await setPlayMode("song")
      ui().setCursorTick(0)
    })
    const seek = vi.spyOn(backend, "transportSeek")
    fireEvent.click(warning()!)
    await flush()
    expect(seek).toHaveBeenCalledWith(6 * BAR)
    expect(ui().cursorTick).toBe(6 * BAR)
    await act(async () => {
      await setPlayMode("pattern")
    })

    // One muted clip fewer and all of them can sound.
    await dispatch({
      type: "updateClips",
      updates: [{ id: added!.created[0], patch: { muted: true } }],
    })
    await flush()
    expect(warning()).toBeNull()
  })
})

describe("the mode notice", () => {
  const mode = () =>
    screen
      .getAllByRole("status")
      .find((item) => item.dataset.slot === "playlist-mode")!

  it("says audio and automation only play in song mode, where it matters", async () => {
    render(<PlaylistPanel />)
    expect(mode()).toHaveTextContent("Play loops the pattern")
    expect(mode()).not.toHaveAttribute("data-notice")

    await addLoop()
    expect(mode()).toHaveTextContent("Pattern loop: audio is silent")
    expect(mode()).toHaveAttribute("data-notice")

    // In song mode it says whether the song plays, not only the mode.
    await act(async () => {
      await setPlayMode("song")
    })
    expect(mode()).toHaveTextContent("Song stopped")
    expect(mode()).not.toHaveAttribute("data-notice")
  })
})

describe("stretch and tempo controls", () => {
  it("applies independent stretch and pitch as one reversible edit", async () => {
    render(<PlaylistPanel />)
    const id = await addLoop()
    const before = project().playlist.clips.find((c) => c.id === id)!
    const count = steps()
    fireEvent.click(screen.getByRole("button", { name: "Stretch / tempo" }))
    await flush()
    fireEvent.click(screen.getByRole("button", { name: "Independent stretch" }))
    await flush()
    fireEvent.change(screen.getByLabelText("Duration multiplier"), {
      target: { value: "1.5" },
    })
    fireEvent.change(screen.getByLabelText("Pitch (semitones)"), {
      target: { value: "7" },
    })
    fireEvent.click(screen.getByRole("button", { name: "Apply to 1 clip" }))
    await flush()
    expect(audio(id)).toMatchObject({
      pitch: 7,
      stretch: {
        mode: "spectral",
        ratio: 1.5,
        quality: "standard",
        formants: false,
      },
    })
    expect(project().playlist.clips.find((c) => c.id === id)?.length).toBe(
      Math.round(before.length * 1.5)
    )
    expect(steps()).toBe(count + 1)
    await act(async () => {
      await undo()
      await settle()
    })
    expect(project().playlist.clips.find((c) => c.id === id)).toEqual(before)
  })
  it("shows invalid ratios and unavailable detection without editing", async () => {
    render(<PlaylistPanel />)
    const id = await addLoop()
    const before = audio(id)
    fireEvent.click(screen.getByRole("button", { name: "Stretch / tempo" }))
    await flush()
    fireEvent.click(screen.getByRole("button", { name: "Detect tempo" }))
    await flush()
    expect(screen.getByRole("alert")).toHaveTextContent(
      "requires the desktop app"
    )
    fireEvent.click(screen.getByRole("button", { name: "Independent stretch" }))
    await flush()
    fireEvent.change(screen.getByLabelText("Duration multiplier"), {
      target: { value: "5" },
    })
    fireEvent.click(screen.getByRole("button", { name: "Apply to 1 clip" }))
    await flush()
    expect(screen.getByRole("alert")).toHaveTextContent("between 0.25 and 4")
    expect(audio(id)).toEqual(before)
  })
  it("fits explicit source BPM to project tempo before Apply", async () => {
    render(<PlaylistPanel />)
    const id = await addLoop()
    fireEvent.click(screen.getByRole("button", { name: "Stretch / tempo" }))
    await flush()
    fireEvent.change(screen.getByLabelText("Source BPM"), {
      target: { value: "90" },
    })
    fireEvent.click(
      screen.getByRole("button", {
        name: `Fit to ${project().settings.tempoBpm} BPM`,
      })
    )
    await flush()
    // Shown to four decimals; Apply uses the exact multiplier.
    const exact = 90 / project().settings.tempoBpm
    expect(screen.getByLabelText("Duration multiplier")).toHaveValue(
      Number(exact.toFixed(4))
    )
    fireEvent.click(screen.getByRole("button", { name: "Apply to 1 clip" }))
    await flush()
    expect(audio(id).stretch).toMatchObject({ mode: "spectral", ratio: exact })
  })
})
