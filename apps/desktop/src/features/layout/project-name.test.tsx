import { act, render, renderHook, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import { ExportDialog } from "@/features/export/export-dialog"
import { exportedName } from "@/features/export/names"
import { runAction } from "@/lib/actions"
import { confirmDiscardChanges, projectName } from "@/lib/flows/project"
import type { Backend } from "@/lib/ipc"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { usePromptStore } from "@/lib/store/prompts"
import { projectDisplayName } from "@/lib/store/selectors"
import { useUiStore } from "@/lib/store/ui"
import { settle, startTestApp } from "@/test/harness"
import { setLanguage } from "@/lib/i18n"

import { TitleBar } from "./title-bar"
import { useWindowTitle } from "./use-app-boot"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let backend: Backend
let stop: () => void

beforeEach(async () => {
  ;({ backend, stop } = await startTestApp())
})
afterEach(() => { stop(); setLanguage("en") })

const project = () => useProjectStore.getState().project

describe("the project's name", () => {
  it("retranslates a detached native title immediately and preserves authored project names", async () => {
    const setTitle = vi.spyOn(backend, "setWindowTitle")
    const panel = renderHook(() => useWindowTitle("Mixer"))
    await waitFor(() => expect(setTitle).toHaveBeenLastCalledWith("Mixer - Windfall"))
    const before = project()
    act(() => setLanguage("es"))
    await waitFor(() => expect(setTitle).toHaveBeenLastCalledWith("Mezclador - Windfall"))
    expect(project()).toBe(before)
    panel.unmount()
    useProjectStore.setState({ path: "/projects/Save.windfall" })
    renderHook(() => useWindowTitle())
    await waitFor(() => expect(setTitle).toHaveBeenLastCalledWith("Save - Windfall"))
    act(() => setLanguage("en"))
    await waitFor(() => expect(setTitle).toHaveBeenLastCalledWith("Save - Windfall"))
    expect(project()).toBe(before)
  })
  it("is the file's name once the project has a file", () => {
    const named = (path: string | null, name = "Demo beat") =>
      projectDisplayName({
        path,
        project: { ...project(), settings: { ...project().settings, name } },
      })
    expect(named(null)).toBe("Demo beat")
    expect(named(null, "")).toBe("Untitled")
    expect(named("/projects/beat1.windfall", "Untitled")).toBe("beat1")
    expect(named("C:\\Music\\My Songs\\beat 2.windfall")).toBe("beat 2")
    expect(named("/projects/v1.2.windfall")).toBe("v1.2")
  })

  it("follows a save in the title bar, the window title and the prompts", async () => {
    const setTitle = vi.spyOn(backend, "setWindowTitle")
    render(
      <TooltipProvider>
        <TitleBar />
      </TooltipProvider>
    )
    renderHook(() => useWindowTitle())
    expect(screen.getByText("Demo beat")).toBeVisible()

    // Saved under another name than the one in the project's settings.
    vi.spyOn(backend, "pickProjectSavePath").mockResolvedValue(
      "/projects/beat1.windfall"
    )
    await runAction("file.saveAs")
    await settle()

    expect(project().settings.name).toBe("Demo beat")
    expect(await screen.findByText("beat1")).toBeVisible()
    expect(projectName()).toBe("beat1")
    await waitFor(() =>
      expect(setTitle).toHaveBeenLastCalledWith("beat1 - Windfall")
    )

    await dispatch({ type: "addPattern" })
    await waitFor(() =>
      expect(setTitle).toHaveBeenLastCalledWith("beat1* - Windfall")
    )
    const asked = confirmDiscardChanges()
    await settle()
    expect(usePromptStore.getState().confirm?.title).toBe(
      'Save changes to "beat1"?'
    )
    usePromptStore.getState().confirm?.resolve(null)
    await asked
  })

  it("is the default name of an exported file", async () => {
    const user = userEvent.setup()
    useProjectStore.setState({ path: "/projects/beat1.windfall" })
    const pick = vi.spyOn(backend, "pickExportPath")
    useUiStore.getState().openDialog("export")
    render(<ExportDialog />)
    await user.click(await screen.findByRole("button", { name: "Choose…" }))
    expect(pick).toHaveBeenCalledWith("beat1", "wav")
    expect(await screen.findByLabelText("Save to")).toHaveValue(
      "/exports/beat1.wav"
    )
  })

  it("names an export the way the shell writes it, with .wav", () => {
    expect(exportedName("/exports/beat1.wav")).toBe("beat1.wav")
    expect(exportedName("C:\\Out\\take 3")).toBe("take 3.wav")
    expect(exportedName("/exports/mix.final.wav")).toBe("mix.final.wav")
  })
})

describe("the name in the title bar", () => {
  it("sits between the menus and the search box and gives way to both", () => {
    render(
      <TooltipProvider>
        <TitleBar />
      </TooltipProvider>
    )
    const title = document.querySelector("[data-slot=project-title]")
    if (!title) throw new Error("the title bar shows no project name")
    // Laid out in the row, so it can never lie on top of a menu.
    expect(title).not.toHaveClass("absolute")
    expect(title).toHaveClass("min-w-0", "flex-1")
    const header = title.parentElement
    const [, menus, name, tools] = [...(header?.children ?? [])]
    expect(name).toBe(title)
    expect(menus).toHaveClass("shrink-0")
    expect(tools).toHaveClass("shrink-0")
    expect(menus.querySelector("[role=menubar]")).not.toBeNull()
  })
})

describe("saving", () => {
  it("takes the unsaved mark from the backend, not from the save having returned", async () => {
    await dispatch({ type: "addPattern" })
    // The shell wrote the file, then saw an edit and kept the project dirty.
    vi.spyOn(backend, "projectSave").mockResolvedValue("/projects/a.windfall")
    await runAction("file.save")
    await settle()
    expect(useProjectStore.getState()).toMatchObject({
      path: "/projects/a.windfall",
      dirty: true,
    })
  })

  it("asks where to save a project without a path, such as an opened backup", async () => {
    const save = vi.spyOn(backend, "projectSave")
    const pick = vi.spyOn(backend, "pickProjectSavePath")
    expect(useProjectStore.getState().path).toBeNull()
    await runAction("file.save")
    await settle()
    expect(pick).toHaveBeenCalledTimes(1)
    expect(save).toHaveBeenCalledWith("/projects/Demo beat.windfall")
    expect(useProjectStore.getState().dirty).toBe(false)
  })
})
