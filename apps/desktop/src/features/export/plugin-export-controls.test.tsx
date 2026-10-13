import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"
import { PluginExportControls } from "./plugin-export-controls"
const invoke = vi.hoisted(() => vi.fn())
vi.mock("@/lib/ipc", () => ({ backend: { exportPluginBundle: invoke }, errorMessage: (error: unknown) => String(error) }))
afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__")
  invoke.mockReset()
})
it("reports desktop capability refusal without simulating a bundle", async () => {
  render(<PluginExportControls />)
  await userEvent.click(screen.getByRole("button", { name: "Export plugin bundles" }))
  expect(screen.getByText("Plugin bundle export requires the desktop app.")).toBeInTheDocument()
  expect(invoke).not.toHaveBeenCalled()
})
it("submits trimmed destinations, reports native failures and permits retry", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true })
  const user = userEvent.setup()
  render(<PluginExportControls />)
  await user.type(screen.getByLabelText("New destination folder"), " C:/exports/song ")
  await user.type(screen.getByLabelText("Windfall plugin runtime file"), " C:/Windfall/windfall_plugin.dll ")
  invoke.mockRejectedValueOnce("Runtime is missing")
  await user.click(screen.getByRole("button", { name: "Export plugin bundles" }))
  expect(await screen.findByText("Runtime is missing")).toBeInTheDocument()
  expect(invoke).toHaveBeenCalledWith("C:/exports/song", "C:/Windfall/windfall_plugin.dll")
  invoke.mockResolvedValueOnce(undefined)
  await user.click(screen.getByRole("button", { name: "Export plugin bundles" }))
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("VST3 and CLAP bundles saved"))
  expect(screen.queryByText("Runtime is missing")).not.toBeInTheDocument()
})
it("automatically resolves the distributed runtime when the override is empty", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true })
  render(<PluginExportControls />)
  await userEvent.type(screen.getByLabelText("New destination folder"), " C:/exports/automatic ")
  invoke.mockResolvedValueOnce(undefined)
  await userEvent.click(screen.getByRole("button", { name: "Export plugin bundles" }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("C:/exports/automatic", ""))
  expect(await screen.findByRole("status")).toHaveTextContent("VST3 and CLAP bundles saved")
})
