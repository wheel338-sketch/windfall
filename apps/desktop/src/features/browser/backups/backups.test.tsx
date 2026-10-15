import { act, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { BrowserEntry } from "@/bindings"
import { openProjectPath } from "@/lib/flows/project"
import { backend } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"

import { BackupsTab } from "."

vi.mock("@/lib/ipc", async () => {
  const { errorSource } = await import("@/lib/ipc/backend")
  return { backend: { browserList: vi.fn() }, errorSource }
})
vi.mock("@/lib/flows/project", () => ({ openProjectPath: vi.fn() }))

const browserList = vi.mocked(backend.browserList)
const older = "Demo 2026-10-06 18-13-05.windfall"
const newer = "Demo 2026-10-07 09-02-01.windfall"

function entry(
  name: string,
  kind: BrowserEntry["kind"] = "project"
): BrowserEntry {
  return { name, path: `C:/songs/Backup/${name}`, kind }
}

beforeEach(() => {
  vi.clearAllMocks()
  browserList.mockReset().mockResolvedValue([])
  useProjectStore.setState({ path: null })
})

describe("project backups browser", () => {
  it("explains an unsaved project has no backups without listing a folder", () => {
    render(<BackupsTab />)
    expect(screen.getByText("No backups")).toBeInTheDocument()
    expect(
      screen.getByText(
        "This project has no backups because it has not been saved."
      )
    ).toBeInTheDocument()
    expect(browserList).not.toHaveBeenCalled()
  })

  it("lists only this project's backups, newest first, ignoring folders and malformed names", async () => {
    useProjectStore.setState({ path: "C:/songs/Demo.windfall" })
    browserList.mockResolvedValue([
      entry(older),
      entry("Other 2026-10-08 12-00-00.windfall"),
      entry(newer),
      entry("Demo.windfall"),
      entry("Demo 2026-10-06 18-13-05.windfall.tmp"),
      entry("Demo 2026-10-06 18:13:05.windfall"),
      entry("Demo 2026-10-06 8-13-05.windfall"),
      entry("Demo  2026-10-06 18-13-05.windfall"),
      entry("Demo 2026-10-08 12-00-00.windfall", "folder"),
    ])
    render(<BackupsTab />)
    const list = await screen.findByRole("list", { name: "Project backups" })
    const rows = within(list).getAllByRole("listitem")
    expect(rows).toHaveLength(2)
    expect(rows[0]).toHaveTextContent(newer)
    expect(rows[1]).toHaveTextContent(older)
    expect(browserList).toHaveBeenCalledExactlyOnceWith("C:/songs/Backup")
  })

  it("delegates Open to the existing project flow with the backup's path", async () => {
    useProjectStore.setState({ path: "C:/songs/Demo.windfall" })
    browserList.mockResolvedValue([entry(older)])
    render(<BackupsTab />)
    await userEvent
      .setup()
      .click(await screen.findByRole("button", { name: `Open ${older}` }))
    expect(openProjectPath).toHaveBeenCalledExactlyOnceWith(entry(older).path)
    expect(
      screen.getByText(
        "Opening a backup opens a copy and does not replace the original file."
      )
    ).toBeInTheDocument()
  })

  it.each(["empty", "missing", "missing (coded)"])(
    "shows no backups for an %s folder",
    async (kind) => {
      useProjectStore.setState({ path: "C:/songs/Demo.windfall" })
      if (kind === "missing") {
        browserList.mockRejectedValue(
          'The folder "C:/songs/Backup" does not exist.'
        )
      }
      if (kind === "missing (coded)") {
        // What the shell sends for a new song: a coded message, with the
        // folder written with Windows separators.
        browserList.mockRejectedValue(
          `windfall-message:${JSON.stringify({
            code: "browser.folderMissing",
            params: { folder: "C:\\songs\\Backup" },
          })}`
        )
      }
      render(<BackupsTab />)
      expect(await screen.findByText("No backups")).toBeInTheDocument()
      expect(
        screen.getByText(/No automatic backups yet/)
      ).toBeInTheDocument()
      expect(screen.getByText(/use the Folder backup tab/)).toBeInTheDocument()
      expect(screen.queryByRole("alert")).toBeNull()
    }
  )

  it("shows list failures without changing the project or opening a file", async () => {
    useProjectStore.setState({ path: "C:/songs/Demo.windfall" })
    const document = useProjectStore.getState()
    browserList.mockRejectedValue(
      new Error("Windfall is not allowed to read this folder.")
    )
    render(<BackupsTab />)
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Windfall is not allowed to read this folder."
    )
    expect(useProjectStore.getState()).toBe(document)
    expect(openProjectPath).not.toHaveBeenCalled()
    expect(screen.queryByText("No backups")).toBeNull()
  })

  it("handles Windows separators and treats punctuation in the stem literally", async () => {
    useProjectStore.setState({ path: "C:\\songs\\Demo [mix].windfall" })
    const backup = "Demo [mix] 2026-10-06 18-13-05.windfall"
    browserList.mockResolvedValue([
      entry(backup),
      entry("Demo m 2026-10-06 18-13-05.windfall"),
    ])
    render(<BackupsTab />)
    expect(await screen.findByText(backup)).toBeInTheDocument()
    expect(screen.getAllByRole("listitem")).toHaveLength(1)
    expect(browserList).toHaveBeenCalledExactlyOnceWith("C:/songs/Backup")
  })

  it("ignores a late listing after switching to an unsaved project", async () => {
    let resolve!: (entries: BrowserEntry[]) => void
    browserList.mockReturnValue(
      new Promise<BrowserEntry[]>((done) => {
        resolve = done
      })
    )
    useProjectStore.setState({ path: "C:/songs/Demo.windfall" })
    render(<BackupsTab />)
    expect(browserList).toHaveBeenCalledTimes(1)
    act(() => useProjectStore.setState({ path: null }))
    await act(async () => resolve([entry(older)]))
    expect(screen.getByText("No backups")).toBeInTheDocument()
    expect(screen.queryByRole("button")).toBeNull()
    expect(browserList).toHaveBeenCalledTimes(1)
  })
})
