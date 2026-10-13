import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { useProjectStore } from "@/lib/store/project"
import { useTransportStore } from "@/lib/store/transport"
import { UpdatesSection } from "./section"
import type { UpdateApi, UpdateProgress } from "./native"

const original = useProjectStore.getState()
afterEach(() => { useProjectStore.setState(original); useTransportStore.setState({ playing: false }); localStorage.removeItem("windfall.update-channel") })
function fixture(): UpdateApi {
  return {
    check: vi.fn(async () => ({ configured: true, reason: null, update: { token: 1, version: "0.2.0", notes: "Release notes", channel: "stable" as const, projectFormat: 1 } })),
    download: vi.fn(async (_token: number, progress: (next: UpdateProgress) => void) => { progress({ downloaded: 3, total: 3, verified: true }) }),
    begin: vi.fn(async () => ({ dirty: useProjectStore.getState().dirty, path: "/project.windfall" })),
    cancel: vi.fn(async () => {}),
    install: vi.fn(async () => {}),
  }
}
describe("signed desktop update surface", () => {
  it("requires verification and freezes before the save/discard/cancel decision", async () => {
    const api = fixture(); useProjectStore.setState({ dirty: false })
    render(<UpdatesSection api={api} />)
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Check for updates" })))
    expect(screen.getByText("Release notes")).toBeVisible()
    const install = screen.getByRole("button", { name: "Install and restart" })
    expect(install).toBeDisabled()
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Download and verify" })))
    expect(screen.getByText("Signature verified. Ready to install.")).toBeVisible()
    act(() => useProjectStore.setState({ dirty: true })); expect(install).toBeEnabled()
    act(() => { useProjectStore.setState({ dirty: false }); useTransportStore.setState({ playing: true }) }); expect(install).toBeEnabled()
    act(() => useTransportStore.setState({ playing: false })); expect(install).toBeEnabled()
    await act(async () => fireEvent.click(install))
    expect(api.begin).toHaveBeenCalledWith(1)
    expect(api.install).toHaveBeenCalledWith(1, "discard", undefined)
    expect(useProjectStore.getState().project).toBe(original.project)
  })
  it("shows offline and corrupt-signature failures, permits retry and never installs", async () => {
    const api = fixture(); vi.mocked(api.check).mockRejectedValueOnce(new Error("Offline"))
    render(<UpdatesSection api={api} />)
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Check for updates" })))
    expect(screen.getByRole("alert")).toHaveTextContent("Offline")
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Check for updates" })))
    vi.mocked(api.download).mockRejectedValueOnce(new Error("Corrupt signature; nothing installed"))
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Download and verify" })))
    expect(screen.getByRole("alert")).toHaveTextContent("Corrupt signature")
    expect(screen.getByRole("button", { name: "Install and restart" })).toBeDisabled()
    expect(api.install).not.toHaveBeenCalled()
  })
  it("does not enable installation when a download completes without verification", async () => {
    const api = fixture(); vi.mocked(api.download).mockResolvedValue(undefined)
    render(<UpdatesSection api={api} />)
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Check for updates" })))
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Download and verify" })))
    expect(screen.getByRole("alert")).toHaveTextContent("not verified")
    expect(api.install).not.toHaveBeenCalled()
  })
  it("reports unconfigured operator values without claiming the app is up to date", async () => {
    const api = fixture(); vi.mocked(api.check).mockResolvedValue({ configured: false, reason: "Missing operator public key", update: null })
    render(<UpdatesSection api={api} />)
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }))
    await waitFor(() => expect(screen.getByText("Missing operator public key")).toBeVisible())
    expect(screen.queryByText("You are up to date.")).toBeNull()
    expect(screen.queryByRole("button", { name: "Install and restart" })).toBeNull()
  })
})

it("cancels the frozen decision and rolls back installer failure before retry", async () => {
  const api=fixture();useProjectStore.setState({dirty:true});render(<UpdatesSection api={api}/>);
  await act(async()=>fireEvent.click(screen.getByRole("button",{name:"Check for updates"})));
  await act(async()=>fireEvent.click(screen.getByRole("button",{name:"Download and verify"})));
  await act(async()=>fireEvent.click(screen.getByRole("button",{name:"Install and restart"})));
  expect(api.begin).toHaveBeenCalledTimes(1);expect(api.install).not.toHaveBeenCalled();
  await act(async()=>fireEvent.click(screen.getByRole("button",{name:"Cancel"})));
  expect(api.cancel).toHaveBeenCalledWith(1);expect(screen.getByRole("button",{name:"Install and restart"})).toBeEnabled();
  await act(async()=>fireEvent.click(screen.getByRole("button",{name:"Install and restart"})));
  vi.mocked(api.install).mockRejectedValueOnce(new Error("Installer refused"));
  await act(async()=>fireEvent.click(screen.getByRole("button",{name:"Save and install"})));
  expect(api.install).toHaveBeenCalledWith(1,"save","/project.windfall");
  expect(screen.getByRole("alert")).toHaveTextContent("Installer refused");
  expect(api.cancel).toHaveBeenCalledTimes(2);
});
