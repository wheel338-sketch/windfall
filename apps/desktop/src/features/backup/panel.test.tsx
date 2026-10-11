import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { BackupVersion } from "@/bindings"
import { setBackend } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { startTestApp } from "@/test/harness"
import { FolderBackupPanel } from "./panel"
import { backupAfterSave, prepareBackupAfterSave, useBackupConfig, useBackupJob } from "./store"

const version: BackupVersion = { id: "a".repeat(64), parent: null, checksum: "b".repeat(64), bytes: 42, createdMs: 1000 }
describe("folder backup panel", () => {
  let app: Awaited<ReturnType<typeof startTestApp>>
  beforeEach(async () => { app = await startTestApp(); useBackupConfig.setState({ projects: {} }); useBackupJob.setState({ busy: false, error: null, result: null }) })
  afterEach(() => { app.stop() })
  it("chooses a folder and backs up only after explicit user action", async () => {
    const backupSave = vi.fn(async () => version)
    setBackend({ ...app.backend, kind: "tauri", pickFolder: async () => "/backup", backupVersions: async () => [], loopCatalog: async () => ({ generation: 17, revision: 0, entries: [] }), backupSave })
    render(<FolderBackupPanel />)
    expect(backupSave).not.toHaveBeenCalled()
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Choose backup folder" }))
    await waitFor(() => expect(screen.getByRole("button", { name: "Back up now" })).toBeEnabled())
    expect(backupSave).not.toHaveBeenCalled()
    await user.click(screen.getByRole("button", { name: "Back up now" }))
    await waitFor(() => expect(backupSave).toHaveBeenCalledWith("/backup", null, 20, 17))
    expect(await screen.findByRole("status")).toHaveTextContent("Portable backup")
  })
  it("reports history conflicts and allows cancellation of an active job", async () => {
    useProjectStore.setState({ path: "/song.windfall" })
    useBackupConfig.setState({ projects: { "/song.windfall": { folder: "/backup", keep: 2, afterSave: false, head: version.id } } })
    const backupCancel = vi.fn(async () => {})
    const backupSave = vi.fn(async () => { throw new Error("Backup conflict: folder history changed.") })
    setBackend({ ...app.backend, kind: "tauri", backupVersions: async () => [version], backupSave, backupCancel, loopCatalog: async () => ({ generation: 17, revision: 0, entries: [] }) })
    render(<FolderBackupPanel />)
    await userEvent.setup().click(screen.getByRole("button", { name: "Back up now" }))
    expect(await screen.findByText("Backup conflict: folder history changed.")).toBeInTheDocument()
    useBackupJob.setState({ busy: true })
    await waitFor(() => expect(screen.getByRole("button", { name: "Cancel backup" })).toBeEnabled())
    await userEvent.setup().click(screen.getByRole("button", { name: "Cancel backup" }))
    expect(backupCancel).toHaveBeenCalledOnce()
  })
  it("opt-in after-save backup carries the reviewed head and original generation", async () => {
    useProjectStore.setState({ path: "/song.windfall" })
    const config = { folder: "/backup", keep: 3, afterSave: false, head: version.id }
    useBackupConfig.setState({ projects: { "/song.windfall": config } })
    const backupSave = vi.fn(async () => ({ ...version, id: "c".repeat(64) }))
    setBackend({ ...app.backend, kind: "tauri", backupSave, loopCatalog: async () => ({ generation: 9, revision: 0, entries: [] }) })
    expect(await prepareBackupAfterSave()).toBeNull()
    useBackupConfig.setState({ projects: { "/song.windfall": { ...config, afterSave: true } } })
    const ticket = await prepareBackupAfterSave()
    await backupAfterSave(ticket, "/other.windfall")
    expect(backupSave).not.toHaveBeenCalled()
    await backupAfterSave(ticket, "/song.windfall")
    expect(backupSave).toHaveBeenCalledWith("/backup", version.id, 3, 9)
    expect(useBackupConfig.getState().projects["/song.windfall"].head).toBe("c".repeat(64))
  })
})
