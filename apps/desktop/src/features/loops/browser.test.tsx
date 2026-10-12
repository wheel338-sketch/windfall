import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { LoopCatalog } from "@/bindings"
import { setBackend } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { startTestApp } from "@/test/harness"
import { LoopBrowser } from "./browser"
import { LOOP_DRAG_TYPE } from "./drag"

describe("own loop browser", () => {
  let app: Awaited<ReturnType<typeof startTestApp>>
  beforeEach(async () => { app = await startTestApp() })
  afterEach(() => { app.stop() })
  function catalog(): LoopCatalog {
    return { generation: 19, revision: useProjectStore.getState().revision, entries: [
      { id: "house-a", name: "Afterglow", genre: "House", tempo: 124, key: 9, mode: "minor", bars: 2, provenance: "Windfall loops-v1 CC0-1.0; no external audio" },
      { id: "ambient-c", name: "Slow Horizon", genre: "Ambient", tempo: 75, key: 0, mode: "minor", bars: 2, provenance: "Windfall loops-v1 CC0-1.0; no external audio" },
    ] }
  }
  it("filters source tempos, auditions and produces a guarded fitted drop payload", async () => {
    const loopPreview = vi.fn(async () => {})
    setBackend({ ...app.backend, kind: "tauri", loopCatalog: async () => catalog(), loopPreview })
    render(<LoopBrowser />)
    await screen.findByRole("button", { name: "Audition Afterglow" })
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Audition Afterglow" }))
    expect(loopPreview).toHaveBeenCalledWith("house-a")
    fireEvent.change(screen.getByLabelText("Minimum BPM"), { target: { value: "100" } })
    expect(screen.queryByRole("button", { name: "Insert Slow Horizon" })).not.toBeInTheDocument()
    const setData = vi.fn()
    const row = screen.getByRole("button", { name: "Insert Afterglow" }).closest("li")
    expect(row).not.toBeNull()
    fireEvent.dragStart(row!, { dataTransfer: { setData, effectAllowed: "" } })
    expect(setData).toHaveBeenCalledWith(LOOP_DRAG_TYPE, JSON.stringify({ id: "house-a", name: "Afterglow", bars: 2, key: null, generation: 19, revision: catalog().revision }))
  })
  it("inserts with native generation and revision as one backend operation", async () => {
    const loopInsert = vi.fn(async () => { throw new Error("Synthetic insertion failure") })
    setBackend({ ...app.backend, kind: "tauri", loopCatalog: async () => catalog(), loopInsert })
    render(<LoopBrowser />)
    await userEvent.setup().click(await screen.findByRole("button", { name: "Insert Afterglow" }))
    await waitFor(() => expect(loopInsert).toHaveBeenCalledWith("house-a", null, null, 0, 19, catalog().revision))
  })
})
