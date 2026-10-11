import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { ExportProgress, MasteringReview } from "@/bindings"
import type { Backend } from "@/lib/ipc"
import { useUiStore } from "@/lib/store/ui"
import { settle, startTestApp } from "@/test/harness"

import { ExportDialog } from "./export-dialog"
import {
  BAD_CEILING,
  BAD_TARGET,
  DEFAULT_MASTERING_DRAFT,
  formatLevel,
  masteringRequest,
  NOT_WITH_STEMS,
  parseLevel,
  presetOf,
  reviewRows,
  reviewSummary,
  reviewVerdict,
} from "./mastering"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

const REVIEW: MasteringReview = {
  before: {
    integratedLufs: -23.4,
    loudnessRangeLu: 6.2,
    momentaryMaxLufs: -19,
    shortTermMaxLufs: -21.1,
    truePeakDbtp: -6.5,
    samplePeakDb: -6.9,
  },
  after: {
    integratedLufs: -14.1,
    loudnessRangeLu: 5.1,
    momentaryMaxLufs: -10.2,
    shortTermMaxLufs: -12.3,
    truePeakDbtp: -1.02,
    samplePeakDb: -1.3,
  },
  targetLufs: -14,
  ceilingDbtp: -1,
  gainDb: 9.6,
  compressionDb: 2.4,
  limitingDb: 1.8,
  passes: 3,
  targetReached: true,
}

describe("mastering request and review figures", () => {
  it("sends nothing while mastering is off, whatever the fields hold", () => {
    expect(
      masteringRequest({ ...DEFAULT_MASTERING_DRAFT, target: "nonsense" }, true)
    ).toEqual({
      options: undefined,
      targetError: null,
      ceilingError: null,
      stemsError: null,
    })
  })

  it("parses the two levels inside their ranges and refuses the rest", () => {
    const on = { ...DEFAULT_MASTERING_DRAFT, enabled: true }
    expect(masteringRequest(on, false).options).toEqual({
      targetLufs: -14,
      ceilingDbtp: -1,
      dynamics: 0.5,
    })
    expect(parseLevel("−9,5", { min: -30, max: -6 })).toBe(-9.5)
    for (const target of ["", "-5.9", "-30.1", "loud", "-1e1", "NaN"]) {
      const request = masteringRequest({ ...on, target }, false)
      expect(request.options, target).toBeUndefined()
      expect(request.targetError, target).toBe(BAD_TARGET)
      expect(request.ceilingError, target).toBeNull()
    }
    for (const ceiling of ["0.1", "-9.5", "x"]) {
      expect(masteringRequest({ ...on, ceiling }, false).ceilingError).toBe(
        BAD_CEILING
      )
    }
    expect(masteringRequest({ ...on, ceiling: "0" }, false).options).toEqual({
      targetLufs: -14,
      ceilingDbtp: 0,
      dynamics: 0.5,
    })
    const stems = masteringRequest(on, true)
    expect(stems.options).toBeUndefined()
    expect(stems.stemsError).toBe(NOT_WITH_STEMS)
  })

  it("names the preset a pair of levels is, and custom otherwise", () => {
    expect(presetOf({ target: "-14", ceiling: "-1" })).toBe("streaming")
    expect(presetOf({ target: "-14.0", ceiling: "-1.00" })).toBe("streaming")
    expect(presetOf({ target: "-9", ceiling: "-0.3" })).toBe("loud")
    expect(presetOf({ target: "-14", ceiling: "-2" })).toBe("custom")
    expect(presetOf({ target: "", ceiling: "-1" })).toBe("custom")
  })

  it("lays out before, after and change for every measure", () => {
    const rows = reviewRows(REVIEW)
    expect(rows.map((row) => row.id)).toEqual([
      "integratedLufs",
      "truePeakDbtp",
      "samplePeakDb",
      "shortTermMaxLufs",
      "loudnessRangeLu",
      "peakToLoudness",
    ])
    expect(rows[0]).toEqual({
      id: "integratedLufs",
      label: "Loudness",
      before: "−23.4 LUFS",
      after: "−14.1 LUFS",
      change: "+9.3 LU",
    })
    expect(rows[1]).toMatchObject({
      before: "−6.5 dBTP",
      after: "−1.0 dBTP",
      change: "+5.5 dB",
    })
    expect(rows[4]).toMatchObject({
      before: "6.2 LU",
      after: "5.1 LU",
      change: "−1.1 LU",
    })
    // Peak to loudness: 16.9 LU before, 13.1 LU after.
    expect(rows[5]).toMatchObject({
      before: "16.9 LU",
      after: "13.1 LU",
      change: "−3.8 LU",
    })
    expect(formatLevel(-120, "LUFS")).toBe("silent")
    expect(formatLevel(0, "dBTP")).toBe("0.0 dBTP")
    expect(reviewSummary(REVIEW)).toBe(
      "+9.6 dB of gain, up to 2.4 dB of band compression, up to 1.8 dB of limiting, in 3 passes."
    )
    expect(
      reviewSummary({ ...REVIEW, gainDb: -3, compressionDb: 0, limitingDb: 0 })
    ).toBe("−3.0 dB of gain, no limiting, in 3 passes.")
  })

  it("says whether the target was reached, and what to change when not", () => {
    expect(reviewVerdict(REVIEW)).toBe(
      "On target: −14.0 LUFS under a ceiling of −1.0 dBTP."
    )
    expect(
      reviewVerdict({
        ...REVIEW,
        targetReached: false,
        targetLufs: -9,
        after: { ...REVIEW.after, integratedLufs: -11.5 },
      })
    ).toMatch(/^2\.5 LU under the target of −9\.0 LUFS/)
    expect(
      reviewVerdict({
        ...REVIEW,
        targetReached: false,
        after: { ...REVIEW.after, truePeakDbtp: -0.4 },
      })
    ).toMatch(/^The true peak is 0\.6 dB over the ceiling/)
  })
})

describe("mastering in the export dialog", () => {
  let backend: Backend
  let stop: () => void
  let progress: ((event: ExportProgress) => void)[]

  beforeEach(async () => {
    vi.clearAllMocks()
    ;({ backend, stop } = await startTestApp())
    progress = []
    vi.spyOn(backend, "onExportProgress").mockImplementation((handler) => {
      progress.push(handler)
      return () => {
        progress = progress.filter((item) => item !== handler)
      }
    })
    useUiStore.getState().openDialog("export")
    render(<ExportDialog />)
  })
  afterEach(() => {
    stop()
    vi.restoreAllMocks()
  })

  const master = () =>
    screen.getByRole("checkbox", { name: "Master the mix to a loudness target" })

  async function choose(
    user: ReturnType<typeof userEvent.setup>,
    label: string,
    option: string
  ) {
    await user.click(screen.getByRole("combobox", { name: label }))
    await user.click(await screen.findByRole("option", { name: option }))
  }

  it("is off until asked for and then sends the chosen targets with the export", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio").mockResolvedValue(undefined)
    expect(master()).not.toBeChecked()
    expect(screen.queryByLabelText("Loudness target (LUFS)")).toBeNull()
    await user.click(screen.getByRole("button", { name: "Choose…" }))
    await user.click(screen.getByRole("button", { name: "Export" }))
    await settle()
    expect(exported).toHaveBeenCalledOnce()
    expect(exported.mock.calls[0][0].mastering).toBeUndefined()

    // Let that export end so the form can be used again.
    const first = exported.mock.calls[0][0]
    act(() => {
      for (const handler of progress) {
        handler({
          path: first.path,
          fraction: 1,
          done: true,
          error: null,
          droppedClips: 0,
          cancelled: true,
        })
      }
    })
    await user.click(master())
    expect(screen.getByLabelText("Loudness target (LUFS)")).toHaveValue(-14)
    expect(screen.getByLabelText("True-peak ceiling (dBTP)")).toHaveValue(-1)
    await choose(user, "Master for", "Loud, −9 LUFS")
    expect(screen.getByLabelText("Loudness target (LUFS)")).toHaveValue(-9)
    expect(screen.getByLabelText("True-peak ceiling (dBTP)")).toHaveValue(-0.3)
    await choose(user, "Dynamics", "Firm")
    await user.click(screen.getByRole("button", { name: "Export" }))
    await settle()
    expect(exported).toHaveBeenCalledTimes(2)
    expect(exported.mock.calls[1][0].mastering).toEqual({
      targetLufs: -9,
      ceilingDbtp: -0.3,
      dynamics: 0.75,
    })
    expect(screen.getByText("Exporting and mastering")).toBeInTheDocument()
  })

  it("stops a target out of range and mastering together with stems before exporting", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio").mockResolvedValue(undefined)
    await user.click(screen.getByRole("button", { name: "Choose…" }))
    await user.click(master())
    const target = screen.getByLabelText("Loudness target (LUFS)")
    await user.clear(target)
    await user.type(target, "-3")
    expect(screen.getByText(BAD_TARGET)).toHaveAttribute("role", "alert")
    expect(screen.getByRole("combobox", { name: "Master for" })).toHaveTextContent(
      "Custom"
    )
    await user.click(screen.getByRole("button", { name: "Export" }))
    await settle()
    expect(exported).not.toHaveBeenCalled()
    expect(target).toHaveFocus()

    await user.clear(target)
    await user.type(target, "-16")
    expect(screen.queryByText(BAD_TARGET)).toBeNull()
    await user.click(
      screen.getByRole("checkbox", { name: "Export mixer tracks as stems" })
    )
    expect(screen.getByText(NOT_WITH_STEMS)).toHaveAttribute("role", "alert")
    await user.click(screen.getByRole("button", { name: "Export" }))
    await settle()
    expect(exported).not.toHaveBeenCalled()

    await user.click(
      screen.getByRole("checkbox", { name: "Export mixer tracks as stems" })
    )
    await user.click(screen.getByRole("button", { name: "Export" }))
    await settle()
    expect(exported).toHaveBeenCalledOnce()
    expect(exported.mock.calls[0][0].stems).toBeUndefined()
    expect(exported.mock.calls[0][0].mastering).toEqual({
      targetLufs: -16,
      ceilingDbtp: -1,
      dynamics: 0.5,
    })
  })

  it("preserves playlist-track export while refusing mastering of separate playlist stems", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio").mockResolvedValue(undefined)
    const playlist = vi.spyOn(backend, "exportPlaylistTracks").mockResolvedValue(undefined)
    await user.click(screen.getByRole("button", { name: "Choose�" }))
    await user.click(screen.getByRole("checkbox", { name: "Export each playlist track to its own file" }))
    await user.click(master())
    expect(screen.getByText(NOT_WITH_STEMS)).toHaveAttribute("role", "alert")
    await user.click(screen.getByRole("button", { name: "Export" }))
    await settle()
    expect(exported).not.toHaveBeenCalled()
    expect(playlist).not.toHaveBeenCalled()
    await user.click(master())
    await user.click(screen.getByRole("button", { name: "Export" }))
    await settle()
    expect(playlist).toHaveBeenCalledOnce()
    expect(playlist.mock.calls[0][0].mastering).toBeUndefined()
    expect(playlist.mock.calls[0][0].mode).toBe("song")
    expect(exported).not.toHaveBeenCalled()
  })

  it("keeps the dialog open on the review of a finished master", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio").mockResolvedValue(undefined)
    await user.click(screen.getByRole("button", { name: "Choose…" }))
    await user.click(master())
    await user.click(screen.getByRole("button", { name: "Export" }))
    await settle()
    const sent = exported.mock.calls[0][0]
    act(() => {
      for (const handler of progress) {
        handler({
          path: sent.path,
          fraction: 0.5,
          done: false,
          error: null,
          droppedClips: 0,
        })
      }
    })
    expect(screen.queryByText("Mastering review")).toBeNull()
    act(() => {
      for (const handler of progress) {
        handler({
          path: sent.path,
          fraction: 1,
          done: true,
          error: null,
          droppedClips: 0,
          mastering: REVIEW,
        })
      }
    })
    expect(toast.success).toHaveBeenCalledOnce()
    expect(useUiStore.getState().dialog).toBe("export")
    const review = screen.getByText("Mastering review").closest("[role=alert]")
    expect(review).not.toBeNull()
    const table = within(review as HTMLElement)
    expect(
      table.getByText("On target: −14.0 LUFS under a ceiling of −1.0 dBTP.")
    ).toBeInTheDocument()
    const loudness = table.getByRole("row", { name: /Loudness −23\.4 LUFS/ })
    expect(within(loudness).getByText("−14.1 LUFS")).toBeInTheDocument()
    expect(within(loudness).getByText("+9.3 LU")).toBeInTheDocument()
    expect(table.getAllByRole("row")).toHaveLength(7)
    expect(table.getByText(/up to 1\.8 dB of limiting, in 3 passes/)).toBeVisible()

    // The form is free again, and Done closes it.
    expect(screen.getByRole("button", { name: "Export" })).toBeEnabled()
    await user.click(screen.getByRole("button", { name: "Done" }))
    await waitFor(() => expect(useUiStore.getState().dialog).not.toBe("export"))
  })

  it("shows what the browser engine says instead of pretending to master", async () => {
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Choose…" }))
    await user.click(master())
    await user.click(screen.getByRole("button", { name: "Export" }))
    expect(
      await screen.findByText(/Mastering requires the desktop audio engine/)
    ).toBeInTheDocument()
    expect(screen.queryByText("Mastering review")).toBeNull()
    expect(toast.success).not.toHaveBeenCalled()
  })
})
