import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { registerMixerActions } from "@/features/mixer/actions"
import { registerPlaylistActions } from "@/features/playlist/actions"
import { installKeymap, registry } from "@/lib/actions"
import { useProjectStore } from "@/lib/store/project"
import { useTransportStore } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { settle, startTestApp } from "@/test/harness"
import { setLanguage } from "@/lib/i18n"

import { CommandPalette, searchActions } from "./command-palette"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let stop: () => void
let uninstall: () => void

beforeEach(async () => {
  ;({ stop } = await startTestApp())
  uninstall = installKeymap()
})
afterEach(() => {
  setLanguage("en")
  uninstall()
  stop()
})

async function openPalette(user: ReturnType<typeof userEvent.setup>) {
  await user.keyboard("{Control>}k{/Control}")
  return screen.findByRole("combobox")
}

const options = () => screen.getAllByRole("option")

describe("CommandPalette", () => {
  it("displays translated commands and ranks Spanish and English names equally without duplicates", async () => {
    setLanguage("es")
    const user = userEvent.setup()
    render(<CommandPalette />)
    const input = await openPalette(user)
    expect(screen.getByRole("dialog", { name: "Paleta de comandos" })).toBeVisible()
    await user.type(input, "guardar como")
    expect(options()[0]).toHaveTextContent("Guardar como")
    await user.clear(input)
    await user.type(input, "save as")
    expect(options()[0]).toHaveTextContent("Guardar como")
    expect(new Set(options().map((row) => row.getAttribute("data-value"))).size).toBe(options().length)
    expect(searchActions(registry.list(), "Export audio", "es")[0].id).toBe("file.export")
    expect(searchActions(registry.list(), "Exportar audio", "es")[0].id).toBe("file.export")
  })
  it("is closed until its shortcut is pressed, and closes on it again", async () => {
    const user = userEvent.setup()
    render(<CommandPalette />)
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument()

    await openPalette(user)
    expect(useUiStore.getState().dialog).toBe("palette")

    await user.keyboard("{Control>}k{/Control}")
    await waitFor(() => expect(useUiStore.getState().dialog).toBeNull())
  })

  it("lists every action with its shortcut", async () => {
    const user = userEvent.setup()
    render(<CommandPalette />)
    await openPalette(user)

    expect(options()).toHaveLength(registry.list().length)
    const save = screen.getByRole("option", { name: /^Save as/ })
    expect(within(save).getByText("Ctrl+Shift+S")).toBeVisible()
    expect(
      within(screen.getByRole("option", { name: /^Play or stop/ })).getByText(
        "Space"
      )
    ).toBeVisible()
    for (const section of ["File", "Edit", "Transport", "Patterns", "View"]) {
      expect(
        screen.getByText(section, { selector: "[cmdk-group-heading]" })
      ).toBeVisible()
    }
  })

  it("shows the FL Studio shortcuts when that keymap is chosen", async () => {
    const user = userEvent.setup()
    useUiStore.getState().setKeymap("fl")
    render(<CommandPalette />)
    await openPalette(user)
    const mixer = screen.getByRole("option", { name: /^Mixer/ })
    expect(within(mixer).getByText("F9")).toBeVisible()
  })

  it("finds actions by loose typing and by keyword", async () => {
    const user = userEvent.setup()
    render(<CommandPalette />)
    const input = await openPalette(user)

    await user.type(input, "sv as")
    expect(options()[0]).toHaveTextContent("Save as")

    await user.clear(input)
    await user.type(input, "bounce")
    expect(options()[0]).toHaveTextContent("Export audio")

    await user.clear(input)
    await user.type(input, "zzzz")
    expect(screen.getByText("No action matches that.")).toBeVisible()
  })

  it("finds an action by a feature it leads to, and suggests the closest actions for unknown words", async () => {
    const user = userEvent.setup()
    const off = registerPlaylistActions()
    render(<CommandPalette />)
    const input = await openPalette(user)

    // The Audio editor button lives in the audio clip settings strip.
    await user.type(input, "audio editor")
    expect(options().map((row) => row.getAttribute("data-value"))).toContain(
      "playlist.clipInspector"
    )
    expect(screen.queryByText("Closest matches")).toBeNull()

    await user.clear(input)
    await user.type(input, "save zzzz")
    expect(screen.getByText("Closest matches")).toBeVisible()
    expect(screen.queryByText("No action matches that.")).toBeNull()
    expect(
      options().some((row) => row.textContent?.startsWith("Save"))
    ).toBe(true)
    off()
  })

  it("lists what a search finds best first, in one list across the sections", async () => {
    const user = userEvent.setup()
    const offs = [registerPlaylistActions(), registerMixerActions()]
    const unregister = () => offs.forEach((off) => off())
    render(<CommandPalette />)
    const input = await openPalette(user)

    await user.type(input, "tall")
    expect(options()[0]).toHaveTextContent("Tall tracks")
    // Each says where it is from, now that the headings are gone.
    expect(options()[0]).toHaveTextContent("Playlist")
    expect(document.querySelector("[cmdk-group-heading]")).toBeNull()
    const tall = options().map((option) => option.textContent)
    expect(tall.some((text) => text?.startsWith("Select all"))).toBe(true)

    await user.type(input, " tracks")
    expect(options()[0]).toHaveTextContent("Tall tracks")
    expect(options()[1]).toHaveTextContent("Unmute all tracks")

    await user.keyboard("{Escape}")
    await waitFor(() => expect(useUiStore.getState().dialog).toBeNull())

    // Opened again, the palette starts over with every section.
    await openPalette(user)
    expect(screen.getByRole("combobox")).toHaveValue("")
    expect(
      screen.getByText("Edit", { selector: "[cmdk-group-heading]" })
    ).toBeVisible()
    unregister()
  })

  it("runs the chosen action and closes", async () => {
    const user = userEvent.setup()
    render(<CommandPalette />)
    const input = await openPalette(user)

    await user.type(input, "play the song{Enter}")
    await settle()
    expect(useTransportStore.getState().mode).toBe("song")
    expect(useUiStore.getState().dialog).toBeNull()
  })

  it("runs an action that is clicked", async () => {
    const user = userEvent.setup()
    render(<CommandPalette />)
    await openPalette(user)
    await user.click(screen.getByRole("option", { name: /^Add channel/ }))
    await settle()
    expect(useProjectStore.getState().project.channels).toHaveLength(5)
  })

  it("shows actions that cannot run right now as disabled", async () => {
    const user = userEvent.setup()
    render(<CommandPalette />)
    await openPalette(user)
    const option = (id: string) =>
      options().find((item) => item.getAttribute("data-value") === id)
    expect(option("edit.undo")).toHaveAttribute("aria-disabled", "true")
    expect(option("file.save")).toHaveAttribute("aria-disabled", "false")
  })
})
