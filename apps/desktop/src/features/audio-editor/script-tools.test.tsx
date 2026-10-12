import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { startTestApp } from "@/test/harness"
import { useProjectStore } from "@/lib/store/project"
import { ScriptTools } from "./script-tools"
import type { AudioScriptPreview } from "./types"

let app: Awaited<ReturnType<typeof startTestApp>>
beforeEach(async () => { app = await startTestApp() })
afterEach(() => { cleanup(); app.stop(); vi.restoreAllMocks() })
const preview = { token: 7, clip: 42, name: "Fixture", frames: 4, sampleRate: 4, peaks: [-0.4, 0.4] }
const response: AudioScriptPreview = { request: 9, token: 7, range: { startFrame: 1, endFrame: 3 }, frames: 2, sampleRate: 4, peaks: [-0.3, 0.3] }
function mount(onCommit = vi.fn()) {
  render(<ScriptTools preview={preview} selection={{ start: 1, end: 3 }} disabled={false} onBusy={() => {}} onCommit={onCommit} />)
  return onCommit
}
it("runs the captured selection, reviews waveform, auditions and applies only the reviewed ticket", async () => {
  Object.defineProperty(app.backend, "kind", { value: "tauri", configurable: true })
  vi.spyOn(app.backend, "scriptBegin").mockResolvedValue(9)
  const run = vi.spyOn(app.backend, "audioScriptPreview").mockResolvedValue(response)
  const audition = vi.spyOn(app.backend, "audioScriptAudition").mockResolvedValue(undefined)
  const apply = vi.spyOn(app.backend, "audioScriptApply").mockRejectedValue(new Error("fixture publication refused"))
  const commit = mount()
  const before = useProjectStore.getState().history.cursor
  fireEvent.click(screen.getByRole("button", { name: "Load Reverse" }))
  fireEvent.click(screen.getByRole("button", { name: "Run script" }))
  await screen.findByText(/Reviewed result: 2 frames/)
  expect(run).toHaveBeenCalledWith({ request: 9, token: 7, range: { startFrame: 1, endFrame: 3 }, source: "reverse", parameters: [] })
  expect(useProjectStore.getState().history.cursor).toBe(before)
  fireEvent.click(screen.getByRole("button", { name: "Audition script result" }))
  await waitFor(() => expect(audition).toHaveBeenCalledWith(7, 9))
  await waitFor(() => expect(screen.getByRole("button", { name: "Apply script result" })).toBeEnabled())
  fireEvent.click(screen.getByRole("button", { name: "Apply script result" }))
  expect(await screen.findByText("fixture publication refused")).toBeVisible()
  expect(apply).toHaveBeenCalledWith(7, 9)
  expect(commit).not.toHaveBeenCalled()
})
it("cancels the native ticket and ignores late preview output", async () => {
  Object.defineProperty(app.backend, "kind", { value: "tauri", configurable: true })
  vi.spyOn(app.backend, "scriptBegin").mockResolvedValue(9)
  let complete!: (value: AudioScriptPreview) => void
  vi.spyOn(app.backend, "audioScriptPreview").mockImplementation(() => new Promise(resolve => { complete = resolve }))
  const cancel = vi.spyOn(app.backend, "scriptCancel").mockResolvedValue(undefined)
  mount()
  fireEvent.click(screen.getByRole("button", { name: "Run script" }))
  await waitFor(() => expect(complete).toBeDefined())
  fireEvent.click(screen.getByRole("button", { name: "Cancel audio script" }))
  expect(cancel).toHaveBeenCalledWith(9)
  await act(async () => complete(response))
  expect(screen.queryByText(/Reviewed result/)).not.toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Apply script result" })).toBeDisabled()
})
it("reports browser capability without creating a scripted audio edit", () => {
  const run = vi.spyOn(app.backend, "audioScriptPreview")
  mount()
  expect(screen.getByRole("button", { name: "Run script" })).toBeDisabled()
  expect(screen.getByText("Audio script processing requires the desktop app.")).toBeVisible()
  expect(run).not.toHaveBeenCalled()
})
