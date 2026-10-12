import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { Backend } from "@/lib/ipc"
import { useUiStore } from "@/lib/store/ui"
import { dispatch } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { settle, startTestApp } from "@/test/harness"

import { ExportDialog } from "./export-dialog"
import { cleanExportPath, parsePatternLoops, parseTailSecs } from "./names"
import {
  selectTimelineRegion,
  useTimelineStore,
} from "@/features/playlist/timeline-store"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let backend: Backend
let stop: () => void

beforeEach(async () => {
  ;({ backend, stop } = await startTestApp())
  useUiStore.getState().openDialog("export")
  render(<ExportDialog />)
})
afterEach(() => stop())

const TAIL_UP_TO = "Keep at most this many seconds after the end"
const TAIL_FIXED = "Seconds to keep after the end, for tails to ring out"

async function exportTo(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "Choose…" }))
  await user.click(screen.getByRole("button", { name: "Export" }))
  await settle()
}

const exportButton = () => screen.getByRole("button", { name: "Export" })

describe("pressing Export", () => {
  it("refuses an edit while the selected export source query is pending", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio")
    await act(async () => {
      await selectTimelineRegion({ start: 17, end: 839 })
    })
    await user.click(screen.getByRole("combobox", { name: "Render" }))
    await user.click(
      await screen.findByRole("option", { name: "The whole song" })
    )
    await user.click(
      screen.getByRole("checkbox", { name: /Export selected song region/ })
    )
    await user.click(screen.getByRole("button", { name: "Choose…" }))
    const source = await backend.timelineState()
    let release = () => {}
    const query = vi.spyOn(backend, "timelineState").mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          release = () => resolve(source)
        })
    )
    await user.click(exportButton())
    expect(query).toHaveBeenCalledOnce()
    await act(async () => {
      await dispatch({
        type: "addMeterChange",
        tick: 4001,
        signature: { numerator: 7, denominator: 8 },
      })
      release()
      await settle()
    })
    expect(exported).not.toHaveBeenCalled()
    expect(
      screen.getByText(/The project changed before the selected region/)
    ).toHaveAttribute("role", "alert")
  })

  it("keeps legacy whole-song export unless the region option is explicitly checked", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio")
    await act(async () => {
      await selectTimelineRegion({ start: 17, end: 839 })
    })
    const sourceQuery = vi.spyOn(backend, "timelineState")
    await user.click(screen.getByRole("combobox", { name: "Render" }))
    await user.click(
      await screen.findByRole("option", { name: "The whole song" })
    )
    expect(
      screen.getByRole("checkbox", { name: /Export selected song region/ })
    ).not.toBeChecked()
    await exportTo(user)
    expect(exported).toHaveBeenCalledWith(
      expect.objectContaining({ mode: "song", region: undefined }),
      expect.stringMatching(/^[0-9a-f-]{36}$/)
    )
    expect(exported.mock.calls[0][0].regionGeneration).toBeUndefined()
    expect(exported.mock.calls[0][0].regionRevision).toBeUndefined()
    expect(sourceQuery).not.toHaveBeenCalled()
  })

  it("exports a copied range and refuses a cleared selection instead of exporting the whole song", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio").mockResolvedValue()
    await act(async () => {
      await selectTimelineRegion({ start: 17, end: 839 })
    })
    await user.click(screen.getByRole("combobox", { name: "Render" }))
    await user.click(
      await screen.findByRole("option", { name: "The whole song" })
    )
    await user.click(
      screen.getByRole("checkbox", { name: /Export selected song region/ })
    )
    await act(async () => {
      await selectTimelineRegion(null)
    })
    await exportTo(user)
    expect(exported).not.toHaveBeenCalled()
    expect(screen.getByText(/The time selection was cleared/)).toHaveAttribute(
      "role",
      "alert"
    )
    await act(async () => {
      await selectTimelineRegion({ start: 17, end: 839 })
    })
    const selected = useTimelineStore.getState().selection
    const canonical = {
      ...(await backend.timelineState()),
      // Backend document generations are independent of the UI replacement epoch.
      generation: getProjectGeneration() + 1000,
    }
    vi.spyOn(backend, "timelineState").mockResolvedValueOnce(canonical)
    await user.click(exportButton())
    const sent = exported.mock.calls[0][0]
    expect(sent.regionGeneration).toBe(canonical.generation)
    expect(sent.regionRevision).toBe(canonical.revision)
    expect(sent.region).toEqual({ start: 17, end: 839 })
    expect(sent.region).not.toBe(selected)
    await act(async () => {
      await selectTimelineRegion({ start: 5, end: 80 })
    })
    expect(sent.region).toEqual({ start: 17, end: 839 })
  })

  it("says a tail is out of range instead of doing nothing", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio")
    await user.click(screen.getByRole("button", { name: "Choose…" }))
    const seconds = screen.getByLabelText(TAIL_UP_TO)
    await user.clear(seconds)
    await user.type(seconds, "45")

    const message = screen.getByText("Enter a number of seconds from 0 to 30.")
    expect(message).toHaveAttribute("role", "alert")
    expect(seconds).toHaveAttribute("aria-invalid", "true")
    expect(seconds).toHaveAccessibleDescription(
      "Enter a number of seconds from 0 to 30."
    )
    // The button stays live, and a press goes to the field that is wrong.
    expect(exportButton()).toBeEnabled()
    await user.click(exportButton())
    await settle()
    expect(exported).not.toHaveBeenCalled()
    expect(seconds).toHaveFocus()

    await user.clear(seconds)
    await user.type(seconds, "30")
    expect(
      screen.queryByText("Enter a number of seconds from 0 to 30.")
    ).toBeNull()
    await user.click(exportButton())
    await settle()
    expect(exported).toHaveBeenCalledWith(
      expect.objectContaining({ tailSecs: 30 }),
      expect.stringMatching(/^[0-9a-f-]{36}$/)
    )
  })

  it("takes a tail that is not a multiple of half a second", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio")
    const seconds = screen.getByLabelText(TAIL_UP_TO)
    await user.clear(seconds)
    await user.type(seconds, "2.3")
    await exportTo(user)
    expect(exported).toHaveBeenCalledWith(
      expect.objectContaining({ tailSecs: 2.3 }),
      expect.stringMatching(/^[0-9a-f-]{36}$/)
    )
  })

  it("says so when the pattern count is out of range, unless the song is rendered", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio")
    const loops = screen.getByLabelText("Times through the pattern")
    await user.clear(loops)
    await user.type(loops, "100")
    expect(screen.getByText("Enter a whole number from 1 to 64.")).toBeVisible()
    await exportTo(user)
    expect(exported).not.toHaveBeenCalled()
    expect(loops).toHaveFocus()

    // The song does not use the count, so it is not in the way there.
    await user.click(screen.getByRole("combobox", { name: "Render" }))
    await user.click(
      await screen.findByRole("option", { name: "The whole song" })
    )
    expect(screen.queryByText("Enter a whole number from 1 to 64.")).toBeNull()
    await user.click(exportButton())
    await settle()
    expect(exported).toHaveBeenCalledWith(
      expect.objectContaining({ mode: "song" }),
      expect.stringMatching(/^[0-9a-f-]{36}$/)
    )
  })

  it("asks for a place to save when there is none", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio")
    expect(exportButton()).toBeEnabled()
    expect(screen.queryByText("Choose where to save the file.")).toBeNull()
    await user.click(exportButton())
    await settle()
    expect(exported).not.toHaveBeenCalled()
    expect(screen.getByText("Choose where to save the file.")).toHaveAttribute(
      "role",
      "alert"
    )
    expect(screen.getByLabelText("Save to")).toHaveFocus()
  })

  it("drops the dots and spaces a typed name ends in before the shell sees them", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio")
    await user.type(screen.getByLabelText("Save to"), "/exports/take 1. ")
    await user.click(exportButton())
    await settle()
    expect(exported).toHaveBeenCalledWith(
      expect.objectContaining({ path: "/exports/take 1" }),
      expect.stringMatching(/^[0-9a-f-]{36}$/)
    )
    // Progress comes back under the path that was sent, and is followed.
    await waitFor(
      () =>
        expect(toast.success).toHaveBeenCalledWith("Exported", {
          description: "take 1.wav",
        }),
      { timeout: 3000 }
    )
    expect(useUiStore.getState().dialog).toBeNull()
  })
})

describe("what is typed into the export form", () => {
  it("loses the dots and spaces at the end of a path", () => {
    expect(cleanExportPath("  /exports/mix.  ")).toBe("/exports/mix")
    expect(cleanExportPath("C:\\Out\\take 3...")).toBe("C:\\Out\\take 3")
    expect(cleanExportPath("/exports/mix.final.wav")).toBe(
      "/exports/mix.final.wav"
    )
    expect(cleanExportPath(" . ")).toBe("")
  })

  it("is a tail from 0 to 30 seconds, in any steps", () => {
    expect(parseTailSecs("0")).toBe(0)
    expect(parseTailSecs("0.3")).toBe(0.3)
    expect(parseTailSecs("30")).toBe(30)
    expect(parseTailSecs("30.5")).toBeNull()
    expect(parseTailSecs("-1")).toBeNull()
    expect(parseTailSecs("")).toBeNull()
    expect(parseTailSecs("abc")).toBeNull()
  })

  it("is a whole number of passes from 1 to 64", () => {
    expect(parsePatternLoops("1")).toBe(1)
    expect(parsePatternLoops("64")).toBe(64)
    expect(parsePatternLoops("65")).toBeNull()
    expect(parsePatternLoops("0")).toBeNull()
    expect(parsePatternLoops("2.5")).toBeNull()
    expect(parsePatternLoops("")).toBeNull()
  })
})

describe("the tail of an export", () => {
  it("stops when the sound has faded, unless told otherwise", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio")
    const auto = screen.getByRole("checkbox", {
      name: "Stop when the tail has faded",
    })
    expect(auto).toBeChecked()
    // With it on, the seconds are an upper limit.
    expect(screen.getByLabelText(TAIL_UP_TO)).toHaveValue(10)
    expect(screen.queryByLabelText(TAIL_FIXED)).toBeNull()

    await exportTo(user)
    expect(exported).toHaveBeenCalledWith(
      expect.objectContaining({ autoTail: true, tailSecs: 10 }),
      expect.stringMatching(/^[0-9a-f-]{36}$/)
    )
  })

  it("keeps a tail of a set length when the option is off", async () => {
    const user = userEvent.setup()
    const exported = vi.spyOn(backend, "exportAudio")
    await user.click(
      screen.getByRole("checkbox", { name: "Stop when the tail has faded" })
    )
    const seconds = screen.getByLabelText(TAIL_FIXED)
    expect(screen.queryByLabelText(TAIL_UP_TO)).toBeNull()
    await user.clear(seconds)
    await user.type(seconds, "2.5")

    await exportTo(user)
    expect(exported).toHaveBeenCalledWith(
      expect.objectContaining({ autoTail: false, tailSecs: 2.5 }),
      expect.stringMatching(/^[0-9a-f-]{36}$/)
    )
  })
})
