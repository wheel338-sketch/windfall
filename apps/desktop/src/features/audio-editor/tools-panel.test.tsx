import { fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { startTestApp } from "@/test/harness"
import type { MockBackend } from "@/lib/ipc/mock"
import { AudioEditorButton } from "./index"
import type { AudioEditPreview } from "./types"

const preview: AudioEditPreview = {
  token: 7,
  clip: 42,
  name: "Take",
  frames: 1000,
  sampleRate: 1000,
  peaks: [-0.5, 0.5, -0.2, 0.2],
}
let backend: MockBackend
let stop: () => void
beforeEach(async () => {
  ;({ backend, stop } = await startTestApp())
})
afterEach(() => stop())

/** Opens the editor on a fixture view and refuses every apply, so it stays open. */
async function open() {
  vi.spyOn(backend, "audioEditorOpen").mockResolvedValue(preview)
  const apply = vi
    .spyOn(backend, "audioEditorApply")
    .mockRejectedValue(new Error("Refused by the fixture."))
  render(<AudioEditorButton clip={42} />)
  fireEvent.click(screen.getByRole("button", { name: "Audio editor" }))
  await screen.findByRole("img", { name: /Clip waveform/ })
  return apply
}

function select(start: number, end: number) {
  fireEvent.change(screen.getByLabelText("Start frame (included)"), {
    target: { value: String(start) },
  })
  fireEvent.change(screen.getByLabelText("End frame (excluded)"), {
    target: { value: String(end) },
  })
}

async function click(name: string) {
  fireEvent.click(screen.getByRole("button", { name }))
  // Each refused apply leaves the editor idle again before the next click.
  await waitFor(() =>
    expect(screen.getByRole("button", { name })).toBeEnabled()
  )
}

describe("audio editor cleaning and processing tools", () => {
  it("groups the tools and sends each one's operation on the selection", async () => {
    const apply = await open()
    expect(screen.getByRole("group", { name: "Clean up" })).toBeInTheDocument()
    expect(screen.getByRole("group", { name: "Process" })).toBeInTheDocument()
    select(100, 900)
    const sent = () => apply.mock.calls.at(-1)?.[0]

    await click("Remove DC offset")
    expect(sent()).toEqual({
      token: 7,
      operation: "removeDc",
      startFrame: 100,
      endFrame: 900,
    })
    await click("De-click")
    expect(sent()?.operation).toEqual({ declick: { strength: 0.5 } })
    await click("Reduce noise")
    expect(sent()?.operation).toEqual({
      reduceNoise: { reductionDb: 12, profile: null },
    })
    await click("Trim silence")
    expect(sent()?.operation).toEqual({ trimSilence: { thresholdDb: -60 } })
    await click("Apply gain")
    expect(sent()?.operation).toEqual({ gain: { db: 3 } })
    await click("Normalize to target")
    expect(sent()?.operation).toEqual({
      normalizeTo: { targetDb: -1, mode: "peak" },
    })
    await click("Fade in with shape")
    expect(sent()?.operation).toEqual({
      fade: { direction: "in", shape: "linear" },
    })
    await click("Fade out with shape")
    expect(sent()?.operation).toEqual({
      fade: { direction: "out", shape: "linear" },
    })
    await click("Swap channels")
    expect(sent()?.operation).toBe("swapChannels")
    await click("Mono sum")
    expect(sent()?.operation).toBe("monoSum")
    await click("Invert phase")
    expect(sent()?.operation).toEqual({ invertPhase: { channels: "both" } })
    await click("Insert silence")
    expect(sent()?.operation).toEqual({ insertSilence: { frames: 100 } })
    expect(sent()?.startFrame).toBe(100)
    expect(sent()?.endFrame).toBe(900)
  })

  it("captures a noise range, sends it as the profile, and clears it again", async () => {
    const apply = await open()
    const cleanup = screen.getByRole("group", { name: "Clean up" })
    expect(within(cleanup).getByRole("status")).toHaveTextContent(
      "No noise profile"
    )
    select(0, 200)
    fireEvent.click(
      screen.getByRole("button", { name: "Use selection as noise profile" })
    )
    expect(within(cleanup).getByRole("status")).toHaveTextContent(
      "Noise profile: frames 0–200 (0.200 s)."
    )
    select(200, 1000)
    fireEvent.change(screen.getByLabelText("Noise reduction (dB)"), {
      target: { value: "20" },
    })
    await click("Reduce noise")
    expect(apply).toHaveBeenLastCalledWith({
      token: 7,
      operation: {
        reduceNoise: {
          reductionDb: 20,
          profile: { startFrame: 0, endFrame: 200 },
        },
      },
      startFrame: 200,
      endFrame: 1000,
    })
    fireEvent.click(screen.getByRole("button", { name: "Clear noise profile" }))
    await click("Reduce noise")
    expect(apply.mock.calls.at(-1)?.[0].operation).toEqual({
      reduceNoise: { reductionDb: 20, profile: null },
    })
  })

  it("disables a tool whose parameter is out of range instead of sending it", async () => {
    const apply = await open()
    fireEvent.change(screen.getByLabelText("Gain (dB)"), {
      target: { value: "0" },
    })
    expect(screen.getByRole("button", { name: "Apply gain" })).toBeDisabled()
    fireEvent.change(screen.getByLabelText("Gain (dB)"), {
      target: { value: "-6" },
    })
    expect(screen.getByRole("button", { name: "Apply gain" })).toBeEnabled()
    fireEvent.change(screen.getByLabelText("De-click strength (%)"), {
      target: { value: "150" },
    })
    expect(screen.getByRole("button", { name: "De-click" })).toBeDisabled()
    expect(screen.getByLabelText("De-click strength (%)")).toHaveAttribute(
      "aria-invalid",
      "true"
    )
    // A stretch that changes nothing is not offered.
    expect(
      screen.getByRole("button", { name: "Stretch and pitch" })
    ).toBeDisabled()
    fireEvent.change(screen.getByLabelText("Length (%)"), {
      target: { value: "150" },
    })
    fireEvent.change(screen.getByLabelText("Pitch (semitones)"), {
      target: { value: "-2" },
    })
    await click("Stretch and pitch")
    expect(apply.mock.calls.at(-1)?.[0].operation).toEqual({
      stretch: {
        ratio: 1.5,
        semitones: -2,
        quality: "standard",
        formants: false,
      },
    })
    expect(apply).toHaveBeenCalledTimes(1)
  })

  it("disables every tool while the selection is not a valid range", async () => {
    await open()
    select(900, 100)
    for (const name of [
      "Remove DC offset",
      "De-click",
      "Reduce noise",
      "Trim silence",
      "Apply gain",
      "Swap channels",
      "Insert silence",
      "Use selection as noise profile",
    ])
      expect(screen.getByRole("button", { name })).toBeDisabled()
  })

  it("shows a native refusal and keeps the editor and its tools available", async () => {
    await open()
    fireEvent.click(screen.getByRole("button", { name: "Mono sum" }))
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Refused by the fixture."
    )
    expect(screen.getByRole("button", { name: "Mono sum" })).toBeEnabled()
    expect(screen.getByRole("img", { name: /Clip waveform/ })).toBeInTheDocument()
  })
})
